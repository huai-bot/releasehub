use super::{models::*, network, selection, Core};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Subscription {
    repository:String, channel:String, preference:String, asset_pattern:Option<String>,
    #[serde(default = "default_management_mode")]
    management_mode:String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Library {
    version: u32, projects: Vec<Subscription>, settings: LibrarySettings,
    #[serde(default)]
    repository_notes: Vec<RepositoryNote>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct LibrarySettings {
    theme: String, check_interval_minutes: u32, notifications: bool, auto_download: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color_scheme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blur: Option<bool>,
    max_concurrent_downloads: usize, close_to_tray: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    download_connections: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    startup_page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    startup_github_tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    navigation_order: Option<Vec<String>>,
}
fn empty_project(owner:String,repo:String,channel:String,preference:String) -> Project {
    let time=now();
    Project { id:uuid::Uuid::new_v4().to_string(), name:repo.clone(),
        url:format!("https://github.com/{owner}/{repo}"),owner,repo,description:String::new(),avatar_url:None,
        channel,preference,management_mode:default_management_mode(),file_status:None,asset_pattern:None,installed_version:None,installed_path:None,installed_source:None,executable_path:None,
        release:None,selected_asset_id:None,status:"notChecked".into(),checked_at:None,error:None,
        default_branch:"main".into(),source_head_sha:None,source_ahead:None,created_at:time }
}
fn metadata(p:&mut Project,raw:&Value)->Result<(),String> {
    if raw.get("private").and_then(Value::as_bool)!=Some(false) { return Err("只支持公开 GitHub 项目".into()); }
    p.name=network::field(raw,"name",200)?;
    p.default_branch=network::field(raw,"default_branch",200)?;
    p.description=raw.get("description").and_then(Value::as_str).unwrap_or("").chars().take(2000).collect();
    p.avatar_url=raw.pointer("/owner/avatar_url").and_then(Value::as_str).filter(|s| {
        url::Url::parse(s).is_ok_and(|u| u.scheme()=="https" && u.host_str()==Some("avatars.githubusercontent.com") && u.username().is_empty() && u.password().is_none())
    }).map(str::to_owned);
    Ok(())
}
impl Core {
    pub(crate) async fn add_project(&self,input:AddProjectInput)->Result<Project,String> {
        validate_choices(&input.channel,&input.preference)?;validate_management_mode(&input.management_mode)?;
        if input.management_mode=="files"&&(input.installed_version.is_some()||input.installed_path.is_some()) {
            return Err("文件项目不能填写已安装版本或路径".into());
        }
        let (owner,repo)=network::repository(&input.repository)?;
        let mut p=empty_project(owner,repo,input.channel,input.preference);
        p.management_mode=input.management_mode;
        if p.management_mode=="files" {p.file_status=Some("notDownloaded".into());}
        let raw=self.github.api(&p.owner,&p.repo,&[],None).await?; metadata(&mut p,&raw)?;
        p.installed_version=input.installed_version.map(|s| bounded_text(&s,200,"已安装版本")).transpose()?;
        p.installed_path=input.installed_path.map(|s| bounded_text(&s,4096,"已安装路径")).transpose()?;
        if p.installed_version.is_some() { p.installed_source=Some("manual".into()); }
        if !self.store.insert_project_unique(&p)? { return Err("项目已在列表中".into()); }
        self.changed(); Ok(p)
    }
    pub(crate) fn update_project(&self,id:&str,changes:ProjectChanges)->Result<Project,String> {
        let installing=self.installing.lock().map_err(|_|"安装状态不可用".to_string())?;
        if installing.contains(id)||self.store.journals()?.iter().any(|j|j.backup.project_id==id) {return Err("正在安装或等待恢复，完成后才能修改项目".into());}
        let _queue=self.download_queue.lock().map_err(|_|"下载队列不可用".to_string())?;
        let downloads=self.store.downloads()?;
        let p=self.store.mutate("projects",id,|p:&mut Project| {
            if let Some(mode)=changes.management_mode {
                validate_management_mode(&mode)?;
                if p.management_mode!=mode {p.selected_asset_id=None;}
                p.management_mode=mode;
            }
            if p.management_mode=="files" {
                if changes.installed_version.as_ref().is_some_and(Option::is_some)||changes.installed_path.as_ref().is_some_and(Option::is_some)||changes.executable_path.as_ref().is_some_and(Option::is_some) {
                    return Err("文件项目不能填写已安装版本或路径".into());
                }
                p.installed_version=None;p.installed_path=None;p.installed_source=None;p.executable_path=None;
            }
            if let Some(c)=changes.channel { p.channel=c; p.release=None;p.selected_asset_id=None; p.status="notChecked".into(); }
            if let Some(v)=changes.preference { p.preference=v; }
            validate_choices(&p.channel,&p.preference)?;
            if let Some(pattern)=changes.asset_pattern {
                p.asset_pattern=pattern.map(|s| bounded_text(&s,200,"文件名规则")).transpose()?;
                if p.management_mode=="files" {p.selected_asset_id=None;}
            }
            if let Some(v)=changes.installed_version {
                p.installed_version=v.map(|s| bounded_text(&s,200,"已安装版本")).transpose()?;
                p.installed_source=p.installed_version.as_ref().map(|_| "manual".into());
            }
            if let Some(path)=changes.installed_path {
                p.installed_path=path.map(|s| bounded_text(&s,4096,"已安装路径")).transpose()?;
                p.executable_path=None;
                if p.installed_version.is_some(){p.installed_source=Some("manual".into());}
            }
            if let Some(executable)=changes.executable_path {
                p.executable_path=executable.map(|s|{
                    let s=bounded_text(&s,4096,"主程序路径")?;
                    self.validate_executable(p,&s).map(|path|path.to_string_lossy().into_owned())
                }).transpose()?;
            }
            if let Some(mut r)=p.release.take() {
                for a in &mut r.assets { selection::classify_for_mode(a,p); }
                p.selected_asset_id=if p.management_mode=="files" {selection::select_for_project(p,&r.assets,p.selected_asset_id)}else{selection::choose(&r.assets)};
                p.release=Some(r);
            }
            if let Some(selected)=changes.selected_asset_id {
                if let Some(id)=selected {
                    if !p.release.as_ref().is_some_and(|r| r.assets.iter().any(|a| a.id==id && if p.management_mode=="files"{a.downloadable}else{a.compatible})) {
                        return Err("附件不适用于当前项目或 Windows x64".into());
                    }
                }
                p.selected_asset_id=selected;
            }
            selection::update_file_status(p,&downloads,&self.data_dir);
            if p.release.is_some() { selection::update_status(p); }
            p.error=None; Ok(())
        })?;
        self.changed(); Ok(p)
    }
    pub(crate) async fn check_updates(self:&Arc<Self>,payload:&Value)->Result<Value,String> {
        let _guard=self.checks.lock().await;
        let allow_auto=match payload.get("allowAutoDownload") { None=>true, Some(Value::Bool(v))=>*v,_=>return Err("自动下载参数无效".into()) };
        let ids:Option<Vec<String>>=match payload.get("projectIds") { None=>None,Some(v)=>Some(super::decode(v.clone())?) };
        if ids.as_ref().is_some_and(|v| v.len()>1000) { return Err("一次最多检查1000个项目".into()); }
        let projects=self.store.projects()?;
        if let Some(ids)=&ids { for id in ids { if !projects.iter().any(|p| &p.id==id) { return Err("要检查的项目不存在".into()); } } }
        let mut checked=0; let mut updates=0; let mut errors=0;
        for mut p in projects.into_iter().filter(|p| ids.as_ref().is_none_or(|v| v.contains(&p.id))) {
            let id=p.id.clone();
            self.store.mutate("projects",&id,|current:&mut Project| {current.status="checking".into();current.error=None;Ok(())})?;
            self.changed();
            let fetched=async {
                let raw=self.github.api(&p.owner,&p.repo,&[],None).await?; metadata(&mut p,&raw)?;
                let release=self.github.release(&p).await?;
                let head=self.github.head(&p.owner,&p.repo,&p.default_branch).await.ok();
                let tag_commit=if let Some(r)=&release {self.github.head(&p.owner,&p.repo,&r.tag).await.ok()}else{None};
                Ok::<_,String>((release,head,tag_commit))
            }.await;
            checked+=1;
            let downloads=self.store.downloads()?;
            let updated=self.store.mutate("projects",&id,|current:&mut Project| {
                current.checked_at=Some(now());
                match fetched {
                    Ok((mut release,head,tag_commit))=> {
                        // A concurrent user change of channel must not receive data from the previous channel.
                        if current.channel!=p.channel {current.status="notChecked".into();return Ok(());}
                        current.name=p.name;current.description=p.description;current.avatar_url=p.avatar_url;current.default_branch=p.default_branch;
                        if let Some(r)=&mut release { for a in &mut r.assets {selection::classify_for_mode(a,current);} }
                        let previous=current.selected_asset_id;
                        current.selected_asset_id=release.as_ref().and_then(|r|selection::select_for_project(current,&r.assets,previous));
                        current.release=release;current.source_ahead=head.as_ref().zip(tag_commit.as_ref()).map(|(a,b)|a!=b);
                        current.source_head_sha=head;current.error=None;
                        selection::update_file_status(current,&downloads,&self.data_dir);selection::update_status(current);
                    }
                    Err(error)=> {current.status="error".into();current.error=Some(error);errors+=1;}
                }
                Ok(())
            });
            let updated=match updated {Ok(p)=>p,Err(e) if self.store.project(&id).is_err()=>{let _=e;continue;},Err(e)=>return Err(e)};
            if updated.status=="updateAvailable" {updates+=1;
                if allow_auto&&self.store.settings()?.auto_download {
                    if let (Some(asset),Some(r))=(updated.selected_asset_id,updated.release.as_ref()) {
                        let exists=self.store.downloads()?.iter().any(|d|d.project_id==updated.id&&d.asset_id==asset&&d.tag==r.tag);
                        if !exists {if let Err(e)=self.queue_download(&updated.id,asset) {
                            self.store.mutate("projects",&id,|p:&mut Project|{p.error=Some(format!("自动下载未开始：{e}"));Ok(())})?;
                        }}
                    }
                }
            }
            self.changed();
        }
        Ok(json!({"checked":checked,"updates":updates,"errors":errors}))
    }
    pub(crate) fn export_library(&self)->Result<String,String> {
        let s=self.store.settings()?;
        let library=Library {version:1,projects:self.store.projects()?.into_iter().map(|p|Subscription{
            repository:format!("{}/{}",p.owner,p.repo),channel:p.channel,preference:p.preference,asset_pattern:p.asset_pattern,management_mode:p.management_mode}).collect(),
            repository_notes:self.store.repository_notes()?,
            settings:LibrarySettings{theme:s.theme,color_scheme:Some(s.color_scheme),material:Some(s.material),blur:Some(s.blur),check_interval_minutes:s.check_interval_minutes,notifications:s.notifications,
                auto_download:s.auto_download,max_concurrent_downloads:s.max_concurrent_downloads,close_to_tray:s.close_to_tray,download_connections:Some(s.download_connections),
                startup_page:Some(s.startup_page),startup_github_tab:Some(s.startup_github_tab),navigation_order:Some(s.navigation_order)}};
        serde_json::to_string_pretty(&library).map_err(|_|"订阅导出失败".into())
    }
    pub(crate) fn import_library(&self,contents:&str)->Result<Value,String> {
        if contents.len()>1024*1024 {return Err("订阅文件超过1MB限制".into());}
        let library:Library=serde_json::from_str(contents).map_err(|_|"订阅文件格式无效或含不支持的字段".to_string())?;
        if library.version!=1 || library.projects.len()>1000 || library.repository_notes.len()>1000 {return Err("订阅文件版本无效或项目、备注过多".into());}
        let mut settings=self.store.settings()?;
        settings.theme=library.settings.theme;settings.check_interval_minutes=library.settings.check_interval_minutes;
        if let Some(color)=library.settings.color_scheme {settings.color_scheme=color;}
        if let Some(material)=library.settings.material {settings.material=material;}
        if let Some(blur)=library.settings.blur {settings.blur=blur;}
        settings.notifications=library.settings.notifications;settings.auto_download=library.settings.auto_download;
        settings.max_concurrent_downloads=library.settings.max_concurrent_downloads;settings.close_to_tray=library.settings.close_to_tray;
        if let Some(connections)=library.settings.download_connections {settings.download_connections=connections;}
        if let Some(page)=library.settings.startup_page {settings.startup_page=page;}
        if let Some(tab)=library.settings.startup_github_tab {settings.startup_github_tab=tab;}
        if let Some(order)=library.settings.navigation_order {settings.navigation_order=order;}
        settings.validate()?;
        // Validate everything before any writes; malformed notes cannot partially import subscriptions.
        let mut notes=std::collections::BTreeMap::new();
        for note in library.repository_notes {
            let note=note.normalized()?;
            if note.name.is_empty()&&note.description.is_empty(){continue;}
            let replace=notes.get(&note.repository).is_none_or(|old:&RepositoryNote|
                chrono::DateTime::parse_from_rfc3339(&note.updated_at).ok()>chrono::DateTime::parse_from_rfc3339(&old.updated_at).ok());
            if replace {notes.insert(note.repository.clone(),note);}
        }
        let mut ready=Vec::new();
        for input in library.projects {
            validate_choices(&input.channel,&input.preference)?;validate_management_mode(&input.management_mode)?;
            let (owner,repo)=network::repository(&input.repository)?;
            let mut p=empty_project(owner,repo,input.channel,input.preference);
            p.management_mode=input.management_mode;
            if p.management_mode=="files" {p.file_status=Some("notDownloaded".into());}
            p.asset_pattern=input.asset_pattern.map(|s|bounded_text(&s,200,"文件名规则")).transpose()?;ready.push(p);
        }
        let notes:Vec<_>=notes.into_values().collect();
        let (imported,skipped,notes_imported)=self.store.import_library(&ready,&notes,&settings)?;
        self.changed();Ok(json!({"imported":imported,"skipped":skipped,"notesImported":notes_imported}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn file_mode_and_color_roundtrip_and_old_exports_still_import() {
        let dir=tempfile::tempdir().unwrap();let source=Core::new(dir.path().join("source"),true,Arc::new(|_,_|{})).unwrap();
        let mut project=empty_project("owner".into(),"assets".into(),"stable".into(),"auto".into());project.management_mode="files".into();source.store.put_project(&project).unwrap();
        let mut settings=source.store.settings().unwrap();settings.color_scheme="violet".into();source.store.save_settings(&settings).unwrap();
        let exported=source.export_library().unwrap();let target=Core::new(dir.path().join("target"),true,Arc::new(|_,_|{})).unwrap();target.import_library(&exported).unwrap();
        assert_eq!(target.store.projects().unwrap()[0].management_mode,"files");assert_eq!(target.store.settings().unwrap().color_scheme,"violet");
        let mut old:Value=serde_json::from_str(&exported).unwrap();old["projects"][0].as_object_mut().unwrap().remove("managementMode");old["settings"].as_object_mut().unwrap().remove("colorScheme");
        let legacy=Core::new(dir.path().join("legacy"),true,Arc::new(|_,_|{})).unwrap();legacy.import_library(&old.to_string()).unwrap();
        assert_eq!(legacy.store.projects().unwrap()[0].management_mode,"software");assert_eq!(legacy.store.settings().unwrap().color_scheme,"blue");
        old["settings"]["colorScheme"]=json!("red");assert!(legacy.import_library(&old.to_string()).is_err());
    }
    #[test] fn glass_preferences_export_import_and_preserve_legacy_choices() {
        let dir=tempfile::tempdir().unwrap();
        let source=Core::new(dir.path().join("source"),true,Arc::new(|_,_|{})).unwrap();
        let target=Core::new(dir.path().join("target"),true,Arc::new(|_,_|{})).unwrap();
        let mut settings=source.store.settings().unwrap(); settings.material="mica".into(); settings.blur=false;
        source.store.save_settings(&settings).unwrap();
        let export=source.export_library().unwrap();
        let mut value:Value=serde_json::from_str(&export).unwrap();
        assert_eq!(value["settings"]["material"],"mica"); assert_eq!(value["settings"]["blur"],false);
        target.import_library(&export).unwrap();
        assert_eq!(target.store.settings().unwrap().material,"mica"); assert!(!target.store.settings().unwrap().blur);
        for key in ["material","blur"] {value["settings"].as_object_mut().unwrap().remove(key);}
        target.import_library(&value.to_string()).unwrap();
        assert_eq!(target.store.settings().unwrap().material,"mica"); assert!(!target.store.settings().unwrap().blur);
        let legacy=Core::new(dir.path().join("legacy"),true,Arc::new(|_,_|{})).unwrap();
        legacy.import_library(&value.to_string()).unwrap();
        assert_eq!(legacy.store.settings().unwrap().material,"acrylic"); assert!(legacy.store.settings().unwrap().blur);
        value["settings"]["material"]=json!("unsupported"); value["settings"]["blur"]=json!(true);
        assert!(target.import_library(&value.to_string()).is_err());
        assert_eq!(target.store.settings().unwrap().material,"mica"); assert!(!target.store.settings().unwrap().blur);
    }
    #[test] fn exports_only_subscriptions() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let mut p=empty_project("owner".into(),"repo".into(),"stable".into(),"portable".into());
        p.installed_path=Some("C:\\private\\app".into());p.installed_version=Some("1.2.0".into());core.store.put_project(&p).unwrap();
        let mut settings=core.store.settings().unwrap();settings.auto_download=true;settings.agent_enabled=true;core.store.save_settings(&settings).unwrap();
        core.set_github_token(Some("private-test-token".into()));let export=core.export_library().unwrap();
        assert!(!export.contains("private-test-token"));assert!(!export.contains("installedPath"));assert!(!export.contains("installedVersion"));
        assert!(!export.contains("buildTasks"));assert!(!export.contains("agentPort"));assert!(!export.contains("agentEnabled"));
        assert_eq!(core.import_library(&export).unwrap()["skipped"],1);
        let imported=Core::new(dir.path().join("fresh-import"),true,Arc::new(|_,_|{})).unwrap();
        assert_eq!(imported.import_library(&export).unwrap()["imported"],1);
        assert!(imported.store.settings().unwrap().auto_download);assert!(!imported.store.settings().unwrap().agent_enabled);
        assert!(!imported.github.token_configured());assert!(imported.store.downloads().unwrap().is_empty());
        let restored=&imported.store.projects().unwrap()[0];assert!(restored.installed_path.is_none());assert!(restored.installed_version.is_none());
    }
    #[test] fn notes_and_startup_preferences_export_without_proxy_or_credentials() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().join("source"),true,Arc::new(|_,_|{})).unwrap();
        let note=RepositoryNote { repository:"Owner/Unsubscribed".into(),name:"本地名称".into(),description:"不会发给 GitHub".into(),updated_at:now() };
        core.store.save_repository_note(&note).unwrap();
        let mut settings=core.store.settings().unwrap(); settings.startup_page="github".into(); settings.startup_github_tab="starred".into(); settings.navigation_order.reverse(); settings.download_connections=8; settings.use_external_downloader=true; settings.external_downloader.name="fixture-local-downloader".into();
        settings.proxy=crate::proxy::ProxySettings { mode:"manual".into(),url:"http://127.0.0.1:9".into(),username:"fixture-proxy-user".into() };
        core.store.save_settings(&settings).unwrap(); core.proxy.configure(settings.proxy.clone(),Some("fixture-only-password".into())).unwrap();
        let export=core.export_library().unwrap(); let value:Value=serde_json::from_str(&export).unwrap();
        assert!(value["settings"].get("proxy").is_none()); assert!(!export.contains("fixture-proxy-user")); assert!(!export.contains("fixture-only-password"));
        assert_eq!(value["settings"]["downloadConnections"],8); assert!(value["settings"].get("externalDownloader").is_none()); assert!(value["settings"].get("useExternalDownloader").is_none()); assert!(!export.contains("fixture-local-downloader"));
        assert_eq!(value["repositoryNotes"][0]["repository"],"owner/unsubscribed");
        let target=Core::new(dir.path().join("target"),true,Arc::new(|_,_|{})).unwrap();
        let mut target_settings=target.store.settings().unwrap(); target_settings.proxy.mode="direct".into(); target.store.save_settings(&target_settings).unwrap();
        let result=target.import_library(&export).unwrap(); assert_eq!(result["imported"],0); assert_eq!(result["notesImported"],1);
        assert_eq!(target.store.settings().unwrap().startup_page,"github"); assert_eq!(target.store.settings().unwrap().startup_github_tab,"starred");
        assert_eq!(target.store.settings().unwrap().navigation_order,settings.navigation_order); assert_eq!(target.store.settings().unwrap().proxy.mode,"direct");
        assert_eq!(target.store.settings().unwrap().download_connections,8); assert!(!target.store.settings().unwrap().use_external_downloader);
        assert_eq!(target.store.repository_notes().unwrap()[0].name,"本地名称");
        let mut old=value; old.as_object_mut().unwrap().remove("repositoryNotes");
        for key in ["startupPage","startupGithubTab","navigationOrder","downloadConnections"] {old["settings"].as_object_mut().unwrap().remove(key);}
        target.import_library(&old.to_string()).unwrap(); assert_eq!(target.store.settings().unwrap().startup_page,"github"); assert_eq!(target.store.settings().unwrap().download_connections,8);
        assert_eq!(target.store.repository_notes().unwrap().len(),1);
    }
    #[test] fn imports_keep_newer_notes_and_reject_invalid_notes_before_writing() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let note=RepositoryNote { repository:"owner/repo".into(),name:"最新备注".into(),description:String::new(),updated_at:"2026-01-02T00:00:00Z".into() };
        core.store.save_repository_note(&note).unwrap();
        let mut value:Value=serde_json::from_str(&core.export_library().unwrap()).unwrap();
        value["repositoryNotes"][0]["name"]=json!("旧备注"); value["repositoryNotes"][0]["updatedAt"]=json!("2026-01-01T00:00:00Z");
        core.import_library(&value.to_string()).unwrap(); assert_eq!(core.store.repository_notes().unwrap()[0].name,"最新备注");
        value["projects"]=json!([{"repository":"owner/new","channel":"stable","preference":"auto","assetPattern":null}]);
        value["repositoryNotes"][0]["name"]=json!("超".repeat(81)); assert!(core.import_library(&value.to_string()).is_err());
        assert!(core.store.projects().unwrap().is_empty());
    }
    #[test] fn refreshed_repository_metadata_does_not_touch_local_notes() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let note=RepositoryNote { repository:"owner/repo".into(),name:"我的备注".into(),description:"我自己的描述".into(),updated_at:now() };
        core.store.save_repository_note(&note).unwrap();
        let mut project=empty_project("owner".into(),"repo".into(),"stable".into(),"auto".into());
        metadata(&mut project,&json!({"private":false,"name":"upstream-name","description":"upstream description","default_branch":"main"})).unwrap();
        core.store.put_project(&project).unwrap(); assert_eq!(core.store.repository_notes().unwrap(),vec![note]);
        core.store.remove_project(&project.id).unwrap(); assert_eq!(core.store.repository_notes().unwrap().len(),1);
    }
    #[test] fn import_rejects_secrets_and_unknown_schema() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let mut v:Value=serde_json::from_str(&core.export_library().unwrap()).unwrap();v["token"]=json!("not-a-real-secret");
        assert!(core.import_library(&v.to_string()).is_err());
    }
}
