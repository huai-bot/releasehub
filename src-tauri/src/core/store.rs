use super::models::{now, BackupRecord, InstallJournal, BuildTask, DownloadRecord, Project, RepositoryNote, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Mutex};

const SCHEMA_VERSION:i64=4;
fn migrate(db:&mut Connection)->Result<(),String> {
    let version:i64=db.pragma_query_value(None,"user_version",|r|r.get(0)).map_err(storage_error)?;
    if version>SCHEMA_VERSION {return Err("资料库由更新版本创建，请升级 ReleaseHub 后打开".into());}
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").map_err(storage_error)?;
    let tx=db.transaction().map_err(storage_error)?;
    if version<1 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS preferences (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS download_digests (id TEXT PRIMARY KEY, digest TEXT);
            PRAGMA user_version=1;").map_err(storage_error)?;
    }
    if version<2 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS repository_notes (repository TEXT PRIMARY KEY COLLATE NOCASE,
            name TEXT NOT NULL, description TEXT NOT NULL, updated_at TEXT NOT NULL);
            PRAGMA user_version=2;").map_err(storage_error)?;
    }
    if version<3 {
        tx.execute_batch("UPDATE preferences SET json=json_set(json,'$.colorScheme','blue')
                WHERE id='settings' AND json_type(json,'$.colorScheme') IS NULL;
            UPDATE projects SET json=json_set(json,'$.managementMode','software')
                WHERE json_type(json,'$.managementMode') IS NULL;
            UPDATE downloads SET json=json_set(json,'$.installable',json(CASE
                WHEN json_extract(json,'$.kind') IN ('installer','portable') THEN 'true' ELSE 'false' END))
                WHERE json_type(json,'$.installable') IS NULL;
            UPDATE downloads SET json=json_set(json,'$.installable',json('false'))
                WHERE json_extract(json,'$.projectId') IN (SELECT id FROM projects WHERE json_extract(json,'$.managementMode')='files');").map_err(storage_error)?;
        let projects:Vec<(String,String)>={
            let mut stmt=tx.prepare("SELECT id,json FROM projects").map_err(storage_error)?;
            let rows=stmt.query_map([],|row|Ok((row.get(0)?,row.get(1)?))).map_err(storage_error)?;
            rows.collect::<Result<_,_>>().map_err(storage_error)?
        };
        for (id,json) in projects {
            let mut project:Project=serde_json::from_str(&json).map_err(storage_error)?;
            super::models::validate_management_mode(&project.management_mode)?;
            if project.management_mode=="files" {
                project.installed_version=None;project.installed_path=None;project.installed_source=None;
            }
            if let Some(mut release)=project.release.take() {
                for asset in &mut release.assets {super::selection::classify_for_mode(asset,&project);}
                project.release=Some(release);
            }
            tx.execute("UPDATE projects SET json=?1 WHERE id=?2",params![serde_json::to_string(&project).map_err(storage_error)?,id]).map_err(storage_error)?;
        }
        tx.pragma_update(None,"user_version",3).map_err(storage_error)?;
    }
    if version<4 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS backups (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS install_journals (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            UPDATE projects SET json=json_set(json,'$.executablePath',NULL) WHERE json_type(json,'$.executablePath') IS NULL;
            PRAGMA user_version=4;").map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)
}
pub struct Store { db: Mutex<Connection> }
fn storage_error(_: impl std::fmt::Display) -> String { "本地数据读写失败，请检查数据目录权限和空间".into() }
impl Store {
    pub fn new(path: &Path) -> Result<Self, String> {
        let mut db = Connection::open(path).map_err(storage_error)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(storage_error)?;
        migrate(&mut db)?;
        let store = Self { db: Mutex::new(db) };
        for mut d in store.downloads()? {
            if d.status == "deleted" { store.remove_download(&d.id)?; continue; }
            if ["queued", "downloading"].contains(&d.status.as_str()) {
                d.status = "failed".into(); d.error = Some("上次下载被中断，请重试".into());
                d.updated_at = now(); store.put_download(&d)?;
            } else if d.status == "installing" {
                // Installation began only from an already completed, verified local download.
                // Keep that artifact recoverable, but never infer that installation succeeded.
                d.status = "completed".into(); d.error = Some("上次安装被中断，下载文件保留，请重新确认安装".into());
                d.updated_at = now(); store.put_download(&d)?;
            }
        }
        for mut p in store.projects()? {
            if p.status == "checking" {
                p.status = "notChecked".into(); p.error = None; store.put_project(&p)?;
            }
        }
        Ok(store)
    }
    fn list<T: DeserializeOwned>(&self, table: &str) -> Result<Vec<T>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let mut stmt = db.prepare(&format!("SELECT json FROM {} ORDER BY rowid DESC", table)).map_err(storage_error)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(storage_error)?;
        rows.map(|r| serde_json::from_str(&r.map_err(storage_error)?).map_err(storage_error)).collect()
    }
    fn get<T: DeserializeOwned>(&self, table: &str, id: &str) -> Result<T, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json: Option<String> = db.query_row(&format!("SELECT json FROM {} WHERE id=?1", table), [id], |r| r.get(0)).optional().map_err(storage_error)?;
        serde_json::from_str(&json.ok_or_else(|| "记录不存在，可能已被删除".to_string())?).map_err(storage_error)
    }
    fn put<T: Serialize>(&self, table: &str, id: &str, value: &T) -> Result<(), String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json = serde_json::to_string(value).map_err(storage_error)?;
        db.execute(&format!("INSERT INTO {} (id,json) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json", table), params![id, json]).map_err(storage_error)?;
        Ok(())
    }
    pub fn mutate<T: Serialize + DeserializeOwned>(&self, table: &str, id: &str, f: impl FnOnce(&mut T) -> Result<(), String>) -> Result<T, String> {
        if !["projects", "downloads", "tasks"].contains(&table) { return Err("数据类型无效".into()); }
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let json: Option<String> = tx.query_row(&format!("SELECT json FROM {} WHERE id=?1", table), [id], |r| r.get(0)).optional().map_err(storage_error)?;
        let mut value: T = serde_json::from_str(&json.ok_or_else(|| "记录不存在，可能已被删除".to_string())?).map_err(storage_error)?;
        f(&mut value)?;
        tx.execute(&format!("UPDATE {} SET json=?1 WHERE id=?2", table), params![serde_json::to_string(&value).map_err(storage_error)?, id]).map_err(storage_error)?;
        if table=="projects" && serde_json::to_value(&value).map_err(storage_error)?.get("managementMode").and_then(serde_json::Value::as_str)==Some("files") {
            // Revoke historical installation permission in the same transaction as the mode change.
            tx.execute("UPDATE downloads SET json=json_set(json,'$.installable',json('false')) WHERE json_extract(json,'$.projectId')=?1",[id]).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(value)
    }
    pub fn projects(&self) -> Result<Vec<Project>, String> { self.list("projects") }
    pub fn project(&self, id: &str) -> Result<Project, String> { self.get("projects", id) }
    pub fn put_project(&self, p: &Project) -> Result<(), String> { self.put("projects", &p.id, p) }
    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        self.project(id)?;
        self.db.lock().map_err(storage_error)?.execute("DELETE FROM projects WHERE id=?1", [id]).map_err(storage_error)?;
        Ok(())
    }
    pub fn backup(&self, id: &str) -> Result<Option<BackupRecord>, String> {
        let db=self.db.lock().map_err(storage_error)?;
        let json:Option<String>=db.query_row("SELECT json FROM backups WHERE id=?1",[id],|r|r.get(0)).optional().map_err(storage_error)?;
        json.map(|j|serde_json::from_str(&j).map_err(storage_error)).transpose()
    }
    pub fn put_backup(&self,b:&BackupRecord)->Result<(),String>{self.put("backups",&b.project_id,b)}
    pub fn journals(&self) -> Result<Vec<InstallJournal>, String> { self.list("install_journals") }
    pub fn put_journal(&self, journal:&InstallJournal) -> Result<(), String> { self.put("install_journals", &journal.backup.project_id, journal) }
    pub fn remove_backup(&self,id:&str)->Result<(),String>{
        self.db.lock().map_err(storage_error)?.execute("DELETE FROM backups WHERE id=?1",[id]).map_err(storage_error)?;Ok(())
    }
    pub fn remove_journal(&self,id:&str)->Result<(),String>{
        self.db.lock().map_err(storage_error)?.execute("DELETE FROM install_journals WHERE id=?1",[id]).map_err(storage_error)?;Ok(())
    }
    /// Publish installed metadata, download state, latest backup, and journal removal atomically.
    pub fn finish_install(&self,p:&Project,d:&DownloadRecord,j:&InstallJournal)->Result<(),String>{
        let mut db=self.db.lock().map_err(storage_error)?;let tx=db.transaction().map_err(storage_error)?;
        for (table,id,json) in [("projects",p.id.as_str(),serde_json::to_string(p)),("downloads",d.id.as_str(),serde_json::to_string(d)),
            ("backups",p.id.as_str(),serde_json::to_string(&j.backup))] {
            tx.execute(&format!("INSERT INTO {table}(id,json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json"),params![id,json.map_err(storage_error)?]).map_err(storage_error)?;
        }
        tx.execute("DELETE FROM install_journals WHERE id=?1",[&p.id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
    pub fn finish_restore(&self,p:&Project,d:Option<&DownloadRecord>,id:&str)->Result<(),String>{
        let mut db=self.db.lock().map_err(storage_error)?;let tx=db.transaction().map_err(storage_error)?;
        tx.execute("UPDATE projects SET json=?1 WHERE id=?2",params![serde_json::to_string(p).map_err(storage_error)?,p.id]).map_err(storage_error)?;
        if let Some(d)=d { tx.execute("UPDATE downloads SET json=?1 WHERE id=?2",params![serde_json::to_string(d).map_err(storage_error)?,d.id]).map_err(storage_error)?; }
        tx.execute("DELETE FROM install_journals WHERE id=?1",[id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
    pub fn downloads(&self) -> Result<Vec<DownloadRecord>, String> { self.list("downloads") }
    pub fn download(&self, id: &str) -> Result<DownloadRecord, String> { self.get("downloads", id) }
    pub fn optional_download(&self,id:&str)->Result<Option<DownloadRecord>,String>{
        let db=self.db.lock().map_err(storage_error)?;
        let json:Option<String>=db.query_row("SELECT json FROM downloads WHERE id=?1",[id],|r|r.get(0)).optional().map_err(storage_error)?;
        json.map(|value|serde_json::from_str(&value).map_err(storage_error)).transpose()
    }
    pub fn remove_download(&self,id:&str)->Result<(),String>{
        let mut db=self.db.lock().map_err(storage_error)?;let tx=db.transaction().map_err(storage_error)?;
        tx.execute("DELETE FROM download_digests WHERE id=?1",[id]).map_err(storage_error)?;
        tx.execute("DELETE FROM downloads WHERE id=?1",[id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
    pub fn put_download(&self, d: &DownloadRecord) -> Result<(), String> { self.put("downloads", &d.id, d) }
    pub fn digest(&self, id: &str) -> Result<Option<String>, String> {
        self.db.lock().map_err(storage_error)?.query_row("SELECT digest FROM download_digests WHERE id=?1", [id], |r| r.get(0)).optional().map_err(storage_error).map(Option::flatten)
    }
    pub fn set_digest(&self, id: &str, digest: Option<&str>) -> Result<(), String> {
        self.db.lock().map_err(storage_error)?.execute("INSERT INTO download_digests(id,digest) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET digest=excluded.digest", params![id, digest]).map_err(storage_error)?; Ok(())
    }
    pub fn tasks(&self) -> Result<Vec<BuildTask>, String> { self.list("tasks") }
    pub fn task(&self, id: &str) -> Result<BuildTask, String> { self.get("tasks", id) }
    pub fn put_task(&self, task: &BuildTask) -> Result<(), String> { self.put("tasks", &task.id, task) }
    pub fn repository_notes(&self) -> Result<Vec<RepositoryNote>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let mut stmt = db.prepare("SELECT repository,name,description,updated_at FROM repository_notes ORDER BY repository").map_err(storage_error)?;
        let rows = stmt.query_map([], |row| Ok(RepositoryNote { repository: row.get(0)?, name: row.get(1)?,
            description: row.get(2)?, updated_at: row.get(3)? })).map_err(storage_error)?;
        rows.map(|row| row.map_err(storage_error)).collect()
    }
    pub fn save_repository_note(&self, note: &RepositoryNote) -> Result<Option<RepositoryNote>, String> {
        let note = note.clone().normalized()?;
        let db = self.db.lock().map_err(storage_error)?;
        if note.name.is_empty() && note.description.is_empty() {
            db.execute("DELETE FROM repository_notes WHERE repository=?1", [&note.repository]).map_err(storage_error)?;
            return Ok(None);
        }
        db.execute("INSERT INTO repository_notes(repository,name,description,updated_at) VALUES(?1,?2,?3,?4)
            ON CONFLICT(repository) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=excluded.updated_at",
            params![note.repository, note.name, note.description, note.updated_at]).map_err(storage_error)?;
        Ok(Some(note))
    }
    pub fn import_library(&self, projects: &[Project], notes: &[RepositoryNote], settings: &Settings) -> Result<(usize, usize, usize), String> {
        settings.validate()?;
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let (mut imported, mut skipped, mut notes_imported) = (0, 0, 0);
        for project in projects {
            let exists: i64 = tx.query_row("SELECT count(*) FROM projects WHERE lower(json_extract(json,'$.owner'))=lower(?1) AND lower(json_extract(json,'$.repo'))=lower(?2)",
                params![project.owner, project.repo], |row| row.get(0)).map_err(storage_error)?;
            if exists > 0 { skipped += 1; continue; }
            tx.execute("INSERT INTO projects(id,json) VALUES(?1,?2)", params![project.id, serde_json::to_string(project).map_err(storage_error)?]).map_err(storage_error)?;
            imported += 1;
        }
        for note in notes {
            let note = note.clone().normalized()?;
            if note.name.is_empty() && note.description.is_empty() { continue; }
            let previous: Option<String> = tx.query_row("SELECT updated_at FROM repository_notes WHERE repository=?1", [&note.repository], |row| row.get(0)).optional().map_err(storage_error)?;
            if previous.as_ref().is_some_and(|old| chrono::DateTime::parse_from_rfc3339(old).ok() >= chrono::DateTime::parse_from_rfc3339(&note.updated_at).ok()) { continue; }
            tx.execute("INSERT INTO repository_notes(repository,name,description,updated_at) VALUES(?1,?2,?3,?4)
                ON CONFLICT(repository) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=excluded.updated_at",
                params![note.repository, note.name, note.description, note.updated_at]).map_err(storage_error)?;
            notes_imported += 1;
        }
        tx.execute("INSERT INTO preferences(id,json) VALUES('settings',?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
            [serde_json::to_string(settings).map_err(storage_error)?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok((imported, skipped, notes_imported))
    }
    pub fn settings(&self) -> Result<Settings, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json: Option<String> = db.query_row("SELECT json FROM preferences WHERE id='settings'", [], |r| r.get(0)).optional().map_err(storage_error)?;
        let settings:Settings=match json { Some(j) => serde_json::from_str(&j).map_err(storage_error)?, None => Settings::default() };
        settings.validate()?;Ok(settings)
    }
    pub fn save_settings(&self, s: &Settings) -> Result<(), String> { s.validate()?; self.put("preferences", "settings", s) }
    pub fn insert_project_unique(&self, p: &Project) -> Result<bool, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let exists: i64 = tx.query_row("SELECT count(*) FROM projects WHERE lower(json_extract(json,'$.owner'))=lower(?1) AND lower(json_extract(json,'$.repo'))=lower(?2)", params![p.owner, p.repo], |r| r.get(0)).map_err(storage_error)?;
        if exists > 0 { return Ok(false); }
        tx.execute("INSERT INTO projects(id,json) VALUES(?1,?2)", params![p.id, serde_json::to_string(p).map_err(storage_error)?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?; Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn version_three_migrates_backup_schema_and_executable_without_losing_metadata() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");let store=Store::new(&path).unwrap();
        let p=super::super::models::test_project("fixture","schema");let mut value=serde_json::to_value(&p).unwrap();value.as_object_mut().unwrap().remove("executablePath");
        store.put("projects",&p.id,&value).unwrap();let mut settings=Settings::default();settings.color_scheme="teal".into();store.save_settings(&settings).unwrap();
        store.db.lock().unwrap().execute_batch("DROP TABLE backups;DROP TABLE install_journals;PRAGMA user_version=3;").unwrap();drop(store);
        let migrated=Store::new(&path).unwrap();assert_eq!(migrated.settings().unwrap().color_scheme,"teal");assert!(migrated.project(&p.id).unwrap().executable_path.is_none());
        assert!(migrated.backup(&p.id).unwrap().is_none());assert!(migrated.journals().unwrap().is_empty());
        assert_eq!(migrated.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),4);
        drop(migrated);assert_eq!(Store::new(&path).unwrap().project(&p.id).unwrap().repo,"schema");
    }
    #[test] fn version_two_migrates_old_records_once_without_losing_preferences() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");
        let store=Store::new(&path).unwrap();
        let mut settings=serde_json::to_value(Settings::default()).unwrap();settings.as_object_mut().unwrap().remove("colorScheme");settings["theme"]=serde_json::json!("dark");
        store.put("preferences","settings",&settings).unwrap();
        let mut project=serde_json::to_value(super::super::models::test_project("owner","repo")).unwrap();
        project.as_object_mut().unwrap().remove("managementMode");
        project["release"]=serde_json::json!({"id":1,"tag":"v1","name":"v1","body":"","publishedAt":now(),
            "url":"https://github.com/owner/repo/releases/tag/v1","prerelease":false,"assets":[{"id":1,"name":"model.safetensors","size":10,
                "url":"https://github.com/owner/repo/releases/download/v1/model.safetensors","contentType":"application/octet-stream",
                "digest":null,"kind":"other","compatible":false,"score":0,"reason":"旧分类"}]});
        let id=project["id"].as_str().unwrap().to_owned();store.put("projects",&id,&project).unwrap();
        for kind in ["portable","installer","other"] {
            let value=serde_json::json!({"id":kind,"projectId":id,"projectName":"repo","assetId":1,"fileName":"file",
                "tag":"v1","url":"https://github.com/owner/repo/releases/download/v1/file","kind":kind,
                "totalBytes":1,"receivedBytes":1,"status":"completed","verified":false,"createdAt":now(),"updatedAt":now()});
            store.put("downloads",kind,&value).unwrap();
        }
        store.db.lock().unwrap().pragma_update(None,"user_version",2).unwrap();drop(store);
        let migrated=Store::new(&path).unwrap();
        assert_eq!(migrated.settings().unwrap().color_scheme,"blue");assert_eq!(migrated.settings().unwrap().theme,"dark");
        let migrated_project=migrated.project(&id).unwrap();assert_eq!(migrated_project.management_mode,"software");
        let asset=&migrated_project.release.as_ref().unwrap().assets[0];assert!(asset.downloadable);assert!(!asset.installable);
        for kind in ["portable","installer","other"] {assert_eq!(migrated.download(kind).unwrap().installable,kind!="other");}
        let version:i64=migrated.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get(0)).unwrap();assert_eq!(version,4);
        let mut settings=migrated.settings().unwrap();settings.color_scheme="amber".into();migrated.save_settings(&settings).unwrap();drop(migrated);
        let reopened=Store::new(&path).unwrap();assert_eq!(reopened.settings().unwrap().color_scheme,"amber");
        reopened.db.lock().unwrap().pragma_update(None,"user_version",5).unwrap();drop(reopened);
        assert!(Store::new(&path).err().unwrap().contains("更新版本"));
        let db=Connection::open(path).unwrap();assert_eq!(db.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),5);
    }
    #[test] fn migration_failure_rolls_back_data_and_version() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");let store=Store::new(&path).unwrap();
        store.put("projects","bad",&serde_json::json!({"owner":"incomplete"})).unwrap();
        store.db.lock().unwrap().pragma_update(None,"user_version",2).unwrap();drop(store);
        assert!(Store::new(&path).is_err());
        let db=Connection::open(path).unwrap();assert_eq!(db.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),2);
        let json:String=db.query_row("SELECT json FROM projects WHERE id='bad'",[],|r|r.get(0)).unwrap();assert!(!json.contains("managementMode"));
    }
    #[test] fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap(); let s = Store::new(&dir.path().join("db.sqlite")).unwrap();
        let mut settings = s.settings().unwrap(); settings.auto_download = true;
        s.save_settings(&settings).unwrap(); assert!(s.settings().unwrap().auto_download);
    }
    #[test] fn old_database_settings_and_notes_survive_portable_reopen() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("db.sqlite"); let store = Store::new(&path).unwrap();
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        for key in ["startupPage", "startupGithubTab", "navigationOrder", "proxy"] { old.as_object_mut().unwrap().remove(key); }
        store.put("preferences", "settings", &old).unwrap(); assert_eq!(store.settings().unwrap().startup_page, "home");
        let mut note = RepositoryNote { repository: "Owner/Repo".into(), name: "我的名称".into(), description: "我的描述".into(), updated_at: now() };
        store.save_repository_note(&note).unwrap(); note.repository = "OWNER/REPO".into(); note.name = "新名称".into();
        store.save_repository_note(&note).unwrap(); assert_eq!(store.repository_notes().unwrap().len(), 1);
        let mut settings = store.settings().unwrap(); settings.startup_page = "github".into(); settings.startup_github_tab = "feed".into(); settings.navigation_order.reverse();
        store.save_settings(&settings).unwrap(); drop(store);
        let moved = dir.path().join("portable.sqlite"); std::fs::copy(path, &moved).unwrap();
        let reopened = Store::new(&moved).unwrap();
        assert_eq!(reopened.repository_notes().unwrap()[0].name, "新名称");
        assert_eq!(reopened.settings().unwrap().navigation_order, settings.navigation_order);
        assert_eq!(reopened.settings().unwrap().startup_github_tab, "feed");
        note.name.clear(); note.description.clear(); assert!(reopened.save_repository_note(&note).unwrap().is_none());
        assert!(reopened.repository_notes().unwrap().is_empty());
    }
    #[test]fn restart_keeps_interrupted_work_recoverable(){
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");let store=Store::new(&path).unwrap();
        for status in ["queued","downloading","installing","completed","deleted"]{
            let time=now();store.put_download(&DownloadRecord{id:status.into(),project_id:"project".into(),project_name:"app".into(),asset_id:1,
                file_name:"app.zip".into(),tag:"v1".into(),url:"https://github.com/owner/app/releases/download/v1/app.zip".into(),kind:"portable".into(),installable:true,
                total_bytes:10,received_bytes:4,status:status.into(),path:None,sha256:None,verified:false,error:None,created_at:time.clone(),updated_at:time}).unwrap();
        }
        drop(store);let reopened=Store::new(&path).unwrap();
        for id in ["queued","downloading"]{let record=reopened.download(id).unwrap();assert_eq!(record.status,"failed");assert!(record.error.unwrap().contains("中断"));}
        let interrupted=reopened.download("installing").unwrap();assert_eq!(interrupted.status,"completed");assert!(interrupted.error.unwrap().contains("重新确认安装"));
        assert_eq!(reopened.download("completed").unwrap().status,"completed");
        assert!(reopened.optional_download("deleted").unwrap().is_none());
    }
}
