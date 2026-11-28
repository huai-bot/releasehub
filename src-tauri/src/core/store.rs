use super::models::{now, BackupRecord, InstallJournal, BuildTask, DownloadRecord, Project, RepositoryNote, Settings, UpdateVersion};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Mutex};

#[path = "installation_checks_store.rs"]
mod installation_checks_store;

const SCHEMA_VERSION:i64=6;
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
    if version<5 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS installation_checks (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, download_id TEXT NOT NULL, json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS installation_checks_project ON installation_checks(project_id);
            CREATE INDEX IF NOT EXISTS installation_checks_download ON installation_checks(download_id);
            CREATE TABLE IF NOT EXISTS installation_bindings (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            PRAGMA user_version=5;").map_err(storage_error)?;
    }
    if version<6 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS update_reads (project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            tag TEXT NOT NULL, PRIMARY KEY(project_id,tag)); PRAGMA user_version=6;").map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)
}
pub struct Store { db: Mutex<Connection> }
fn storage_error(_: impl std::fmt::Display) -> String { "本地数据读写失败，请检查数据目录权限和空间".into() }

fn read_project(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<Option<Project>, String> {
    let json: Option<String> = tx.query_row("SELECT json FROM projects WHERE id=?1", [id], |row| row.get(0)).optional().map_err(storage_error)?;
    json.map(|json| serde_json::from_str(&json).map_err(storage_error)).transpose()
}

fn save_project(tx: &rusqlite::Transaction<'_>, p: &Project) -> Result<(), String> {
    tx.execute("UPDATE projects SET json=?1 WHERE id=?2", params![serde_json::to_string(p).map_err(storage_error)?, p.id]).map_err(storage_error)?;
    Ok(())
}

fn detach_repository_downloads(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<Vec<String>, String> {
    let download_ids = {
        let mut query = tx.prepare("SELECT id FROM downloads WHERE json_extract(json,'$.projectId')=?1").map_err(storage_error)?;
        let result = query.query_map([id], |row| row.get(0)).map_err(storage_error)?.collect::<Result<Vec<String>, _>>().map_err(storage_error)?;
        result
    };
    // Completed files are retained as history, but no previous asset can be retried, imported or installed for the new repository.
    tx.execute("UPDATE downloads SET json=json_set(json,'$.projectId','','$.installable',json('false'),'$.updatedAt',?2,
        '$.status',CASE WHEN json_extract(json,'$.status') IN ('queued','downloading','external') THEN 'cancelled' ELSE json_extract(json,'$.status') END,
        '$.error',CASE WHEN json_extract(json,'$.status') IN ('queued','downloading','external') THEN '仓库已更改，旧下载已停止' ELSE json_extract(json,'$.error') END)
        WHERE json_extract(json,'$.projectId')=?1", params![id, now()]).map_err(storage_error)?;
    Ok(download_ids)
}

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
                p.status = if super::local_apps::has_repository(&p) { "notChecked" } else { "local" }.into();
                p.error = None; store.put_project(&p)?;
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
        self.mutate_record(table, id, false, f)
    }
    pub fn mutate_project(&self, id: &str, invalidate_installation: bool, f: impl FnOnce(&mut Project) -> Result<(), String>) -> Result<Project, String> {
        self.mutate_record("projects", id, invalidate_installation, f)
    }
    fn mutate_record<T: Serialize + DeserializeOwned>(&self, table: &str, id: &str, invalidate_installation: bool, f: impl FnOnce(&mut T) -> Result<(), String>) -> Result<T, String> {
        if !["projects", "downloads", "tasks"].contains(&table) { return Err("数据类型无效".into()); }
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let json: Option<String> = tx.query_row(&format!("SELECT json FROM {} WHERE id=?1", table), [id], |r| r.get(0)).optional().map_err(storage_error)?;
        let mut value: T = serde_json::from_str(&json.ok_or_else(|| "记录不存在，可能已被删除".to_string())?).map_err(storage_error)?;
        f(&mut value)?;
        tx.execute(&format!("UPDATE {} SET json=?1 WHERE id=?2", table), params![serde_json::to_string(&value).map_err(storage_error)?, id]).map_err(storage_error)?;
        if table=="projects" {
            let files = serde_json::to_value(&value).map_err(storage_error)?.get("managementMode").and_then(serde_json::Value::as_str)==Some("files");
            if files {
                // Revoke historical installation permission in the same transaction as the mode change.
                tx.execute("UPDATE downloads SET json=json_set(json,'$.installable',json('false')) WHERE json_extract(json,'$.projectId')=?1",[id]).map_err(storage_error)?;
            }
            if files || invalidate_installation { installation_checks_store::invalidate_project(&tx, id)?; }
        }
        if table=="downloads" && serde_json::to_value(&value).map_err(storage_error)?.get("status").and_then(serde_json::Value::as_str)==Some("deleted") {
            installation_checks_store::invalidate_download(&tx,id)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(value)
    }
    pub(super) fn mutate_project_if<T>(&self, id: &str, f: impl FnOnce(&mut Project) -> Option<T>) -> Result<Option<(Project, T)>, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let Some(mut p) = read_project(&tx, id)? else { return Ok(None); };
        let Some(value) = f(&mut p) else { return Ok(None); };
        save_project(&tx, &p)?;
        tx.commit().map_err(storage_error)?;
        Ok(Some((p, value)))
    }

    pub(super) fn insert_local_project(&self, p: &Project) -> Result<bool, String> {
        let executable = p.executable_path.as_deref().ok_or("本地程序路径为空")?;
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        {
            let mut query = tx.prepare("SELECT json_extract(json,'$.executablePath') FROM projects WHERE json_type(json,'$.executablePath')='text'").map_err(storage_error)?;
            let paths = query.query_map([], |r| r.get::<_, String>(0)).map_err(storage_error)?;
            for path in paths {
                if super::local_apps::same_executable_path(executable, &path.map_err(storage_error)?) { return Ok(false); }
            }
        }
        tx.execute("INSERT INTO projects(id,json) VALUES(?1,?2)", params![p.id, serde_json::to_string(p).map_err(storage_error)?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(true)
    }

    // The expected snapshot denotes an automatic match, which may only consume that pending EXE.
    // A manual decision is final even if a repository lookup has already started in the background.
    pub(super) fn bind_project_repository(&self, id: &str, expected: Option<&Project>, mut matched: Option<&Project>, version: Option<String>)
        -> Result<Option<(Project, Vec<String>)>, String>
    {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let Some(mut p) = read_project(&tx, id)? else {
            return if expected.is_some() { Ok(None) } else { Err("项目已删除".into()) };
        };
        if let Some(old) = expected {
            if !super::local_apps::same_pending_executable(&p, old) { return Ok(None); }
            if p.installed_version.is_none() && p.installed_source.is_none() {
                p.installed_version = version;
                if p.installed_version.is_some() { p.installed_source = Some("executable".into()); }
            }
        }
        if let Some(repo) = matched {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id<>?1
                AND json_extract(json,'$.owner')=?2 COLLATE NOCASE AND json_extract(json,'$.repo')=?3 COLLATE NOCASE)",
                params![id, repo.owner, repo.repo], |r| r.get(0)).map_err(storage_error)?;
            if exists {
                if expected.is_none() { return Err("这个 GitHub 仓库已在软件库中".into()); }
                matched = None;
            }
        }
        let linked = super::local_apps::has_repository(&p);
        if matched.is_none() && p.management_mode == "files" { return Err("文件项目必须保留一个 GitHub 仓库".into()); }
        let changed = match matched {
            Some(repo) => !super::local_apps::same_repository(&p, repo),
            None => linked,
        };
        let mut detached = Vec::new();
        if changed {
            p.release = None; p.selected_asset_id = None; p.checked_at = None;
            p.source_head_sha = None; p.source_ahead = None; p.error = None;
            if linked { p.asset_pattern = None; }
            p.file_status = (p.management_mode == "files" && matched.is_some()).then(|| "notDownloaded".into());
            tx.execute("DELETE FROM update_reads WHERE project_id=?1", [id]).map_err(storage_error)?;
            installation_checks_store::invalidate_project(&tx, id)?;
            detached = detach_repository_downloads(&tx, id)?;
        }
        if let Some(repo) = matched {
            p.owner.clone_from(&repo.owner); p.repo.clone_from(&repo.repo); p.url.clone_from(&repo.url);
            p.name.clone_from(&repo.name); p.description.clone_from(&repo.description);
            p.avatar_url.clone_from(&repo.avatar_url); p.default_branch.clone_from(&repo.default_branch);
            if changed { p.status = "notChecked".into(); }
        } else {
            p.owner.clear(); p.repo.clear(); p.url.clear(); p.description.clear(); p.avatar_url = None;
            p.default_branch.clear(); p.status = "local".into(); p.error = None;
            if let Some(name) = p.executable_path.as_deref().and_then(|path| super::local_apps::executable_name(Path::new(path))) {
                p.name = name;
            }
        }
        p.repository_match = Some(match (expected.is_some(), matched.is_some()) {
            (true, true) => "auto", (true, false) => "unmatched", (false, _) => "manual",
        }.into());
        save_project(&tx, &p)?;
        tx.commit().map_err(storage_error)?;
        Ok(Some((p, detached)))
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> { self.list("projects") }
    pub fn current_read_tags(&self) -> Result<std::collections::HashMap<String,String>, String> {
        let db=self.db.lock().map_err(storage_error)?;
        let mut query=db.prepare("SELECT projects.id, update_reads.tag FROM projects JOIN update_reads
            ON project_id=projects.id AND tag=json_extract(projects.json,'$.release.tag')").map_err(storage_error)?;
        let rows=query.query_map([], |row| Ok((row.get(0)?,row.get(1)?))).map_err(storage_error)?;
        rows.collect::<Result<_,_>>().map_err(storage_error)
    }
    pub fn mark_updates_read(&self, versions: &[UpdateVersion]) -> Result<usize,String> {
        let mut db=self.db.lock().map_err(storage_error)?;
        let transaction=db.transaction().map_err(storage_error)?;
        let mut count=0;
        {
            let mut insert=transaction.prepare("INSERT OR IGNORE INTO update_reads(project_id,tag)
                SELECT id,?2 FROM projects WHERE id=?1 AND json_extract(json,'$.release.tag')=?2
                AND NOT coalesce(json_extract(json,'$.updatesMuted'),0)").map_err(storage_error)?;
            for version in versions {
                count+=insert.execute(params![version.project_id,version.tag]).map_err(storage_error)?;
            }
        }
        transaction.commit().map_err(storage_error)?;
        Ok(count)
    }
    pub fn project(&self, id: &str) -> Result<Project, String> { self.get("projects", id) }
    pub fn put_project(&self, p: &Project) -> Result<(), String> { self.put("projects", &p.id, p) }
    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        if tx.execute("DELETE FROM projects WHERE id=?1", [id]).map_err(storage_error)? == 0 { return Err("项目已删除".into()); }
        tx.execute("DELETE FROM installation_checks WHERE project_id=?1", [id]).map_err(storage_error)?;
        tx.execute("DELETE FROM installation_bindings WHERE id=?1", [id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
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
        for (table,id,json) in [("projects",p.id.as_str(),serde_json::to_string(p)),("downloads",d.id.as_str(),serde_json::to_string(d))] {
            tx.execute(&format!("INSERT INTO {table}(id,json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json"),params![id,json.map_err(storage_error)?]).map_err(storage_error)?;
        }
        installation_checks_store::invalidate_project(&tx, &p.id)?;
        if j.snapshot_enabled {
            let json = serde_json::to_string(&j.backup).map_err(storage_error)?;
            tx.execute("INSERT INTO backups(id,json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json",params![p.id,json]).map_err(storage_error)?;
        }
        tx.execute("DELETE FROM install_journals WHERE id=?1",[&p.id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
    pub fn finish_restore(&self,p:&Project,d:Option<&DownloadRecord>,id:&str)->Result<(),String>{
        let mut db=self.db.lock().map_err(storage_error)?;let tx=db.transaction().map_err(storage_error)?;
        tx.execute("UPDATE projects SET json=?1 WHERE id=?2",params![serde_json::to_string(p).map_err(storage_error)?,p.id]).map_err(storage_error)?;
        if let Some(d)=d { tx.execute("UPDATE downloads SET json=?1 WHERE id=?2",params![serde_json::to_string(d).map_err(storage_error)?,d.id]).map_err(storage_error)?; }
        tx.execute("DELETE FROM install_journals WHERE id=?1",[id]).map_err(storage_error)?;
        installation_checks_store::invalidate_project(&tx, id)?;
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
        installation_checks_store::invalidate_download(&tx, id)?;
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
        let mut settings:Settings=match json { Some(j) => serde_json::from_str(&j).map_err(storage_error)?, None => Settings::default() };
        settings.external_downloader.migrate_legacy_ab();
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
    #[test] fn version_four_adds_installation_tables_without_changing_existing_records() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");let store=Store::new(&path).unwrap();
        let p=super::super::models::test_project("fixture","schema-four");store.put_project(&p).unwrap();
        store.db.lock().unwrap().execute_batch("DROP TABLE installation_checks; DROP TABLE installation_bindings; PRAGMA user_version=4;").unwrap();
        drop(store);
        let migrated=Store::new(&path).unwrap();
        assert_eq!(migrated.project(&p.id).unwrap().repo,"schema-four");
        assert!(migrated.installations(None).unwrap().is_empty());
        assert!(migrated.install_bindings(None).unwrap().is_empty());
        assert_eq!(migrated.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION);
    }
    #[test] fn version_three_migrates_backup_schema_and_executable_without_losing_metadata() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db.sqlite");let store=Store::new(&path).unwrap();
        let p=super::super::models::test_project("fixture","schema");let mut value=serde_json::to_value(&p).unwrap();value.as_object_mut().unwrap().remove("executablePath");
        store.put("projects",&p.id,&value).unwrap();let mut settings=Settings::default();settings.color_scheme="teal".into();store.save_settings(&settings).unwrap();
        store.db.lock().unwrap().execute_batch("DROP TABLE backups;DROP TABLE install_journals;PRAGMA user_version=3;").unwrap();drop(store);
        let migrated=Store::new(&path).unwrap();assert_eq!(migrated.settings().unwrap().color_scheme,"teal");assert!(migrated.project(&p.id).unwrap().executable_path.is_none());
        assert!(migrated.backup(&p.id).unwrap().is_none());assert!(migrated.journals().unwrap().is_empty());
        assert_eq!(migrated.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION);
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
        let version:i64=migrated.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get(0)).unwrap();assert_eq!(version,SCHEMA_VERSION);
        let mut settings=migrated.settings().unwrap();settings.color_scheme="amber".into();migrated.save_settings(&settings).unwrap();drop(migrated);
        let reopened=Store::new(&path).unwrap();assert_eq!(reopened.settings().unwrap().color_scheme,"amber");
        reopened.db.lock().unwrap().pragma_update(None,"user_version",SCHEMA_VERSION+1).unwrap();drop(reopened);
        assert!(Store::new(&path).err().unwrap().contains("更新版本"));
        let db=Connection::open(path).unwrap();assert_eq!(db.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION+1);
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
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("db.sqlite"); let mut store = Store::new(&path).unwrap();
        let mut settings = store.settings().unwrap(); settings.auto_download = true;
        for layout in ["two","three","list"] {
            settings.library_layout=layout.into(); store.save_settings(&settings).unwrap(); drop(store);
            store=Store::new(&path).unwrap(); let restored=store.settings().unwrap();
            assert!(restored.auto_download); assert_eq!(restored.library_layout,layout);
        }
    }
    #[test] fn legacy_ab_settings_migrate_on_read_and_persist_after_save() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("db.sqlite"); let store=Store::new(&path).unwrap();
        let mut old=Settings::default(); old.library_layout="two".into(); old.use_external_downloader=true;
        old.external_downloader=super::super::models::ExternalDownloader { name:"AB Download Manager".into(),
            endpoint:"http://127.0.0.1:15151/start-headless-download".into(),
            request_template:r#"{
                "startDownload": true, "name": "{fileName}",
                "downloadSource": { "downloadPage": "{pageUrl}", "link": "{url}" }
            }"#.into() };
        store.put("preferences","settings",&old).unwrap(); drop(store);
        let migrated=Store::new(&path).unwrap(); let settings=migrated.settings().unwrap();
        assert_eq!(settings.library_layout,"two"); assert!(settings.use_external_downloader);
        assert_eq!(settings.external_downloader.endpoint,"http://127.0.0.1:15151/add");
        assert_eq!(serde_json::to_value(&settings.external_downloader).unwrap(),
            serde_json::to_value(super::super::models::ExternalDownloader::default()).unwrap());
        migrated.save_settings(&settings).unwrap(); drop(migrated);
        let reopened=Store::new(&path).unwrap(); let persisted:Settings=reopened.get("preferences","settings").unwrap();
        let expected=serde_json::to_value(&settings).unwrap();
        assert_eq!(serde_json::to_value(persisted).unwrap(),expected);
        assert_eq!(serde_json::to_value(reopened.settings().unwrap()).unwrap(),expected);
    }
    #[test] fn old_database_settings_and_notes_survive_portable_reopen() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("db.sqlite"); let store = Store::new(&path).unwrap();
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        for key in ["startupPage", "startupGithubTab", "navigationOrder", "proxy", "libraryLayout"] { old.as_object_mut().unwrap().remove(key); }
        store.put("preferences", "settings", &old).unwrap(); assert_eq!(store.settings().unwrap().startup_page, "home"); assert_eq!(store.settings().unwrap().library_layout, "three");
        let mut note = RepositoryNote { repository: "Owner/Repo".into(), name: "我的名称".into(), description: "我的描述".into(), updated_at: now() };
        store.save_repository_note(&note).unwrap(); note.repository = "OWNER/REPO".into(); note.name = "新名称".into();
        store.save_repository_note(&note).unwrap(); assert_eq!(store.repository_notes().unwrap().len(), 1);
        let mut settings = store.settings().unwrap(); settings.startup_page = "github".into(); settings.startup_github_tab = "feed".into(); settings.navigation_order.reverse(); settings.library_layout="list".into();
        store.save_settings(&settings).unwrap(); drop(store);
        let moved = dir.path().join("portable.sqlite"); std::fs::copy(path, &moved).unwrap();
        let reopened = Store::new(&moved).unwrap();
        assert_eq!(reopened.repository_notes().unwrap()[0].name, "新名称");
        assert_eq!(reopened.settings().unwrap().navigation_order, settings.navigation_order);
        assert_eq!(reopened.settings().unwrap().startup_github_tab, "feed"); assert_eq!(reopened.settings().unwrap().library_layout, "list");
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
