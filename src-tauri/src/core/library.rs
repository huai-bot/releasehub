use super::{local_apps::{has_repository, same_repository}, models::*, network, selection, Core};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Subscription {
    repository:String, channel:String, preference:String, asset_pattern:Option<String>,
    #[serde(default = "default_management_mode")]
    management_mode:String,
    #[serde(default)]
    backup_enabled:bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    updates_muted:bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    library_layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    library_sort: Option<String>,
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
pub(super) fn empty_project(owner:String,repo:String,channel:String,preference:String) -> Project {
    let time=now();
    let local = owner.is_empty() && repo.is_empty();
    Project { id:uuid::Uuid::new_v4().to_string(), name:repo.clone(),
        url:if local { String::new() } else { format!("https://github.com/{owner}/{repo}") },owner,repo,description:String::new(),avatar_url:None,
        repository_match:None,channel,preference,management_mode:default_management_mode(),file_status:None,asset_pattern:None,installed_version:None,installed_path:None,installed_source:None,executable_path:None,
        backup_enabled:false,updates_muted:false,
        release:None,selected_asset_id:None,status:if local { "local" } else { "notChecked" }.into(),checked_at:None,error:None,
        default_branch:if local { "" } else { "main" }.into(),source_head_sha:None,source_ahead:None,created_at:time }
}
pub(super) fn metadata(p:&mut Project,raw:&Value)->Result<(),String> {
    if raw.get("private").and_then(Value::as_bool)!=Some(false) { return Err("只支持公开 GitHub 项目".into()); }
    p.name=network::field(raw,"name",200)?;
    p.default_branch=network::field(raw,"default_branch",200)?;
    p.description=raw.get("description").and_then(Value::as_str).unwrap_or("").chars().take(2000).collect();
    p.avatar_url=raw.pointer("/owner/avatar_url").and_then(Value::as_str).filter(|s| {
        url::Url::parse(s).is_ok_and(|u| u.scheme()=="https" && u.host_str()==Some("avatars.githubusercontent.com") && u.username().is_empty() && u.password().is_none())
    }).map(str::to_owned);
    Ok(())
}

pub(super) fn update_project_status(p: &mut Project) {
    if !has_repository(p) {
        p.status = "local".into();
    } else if p.management_mode == "software" && p.release.is_some() && p.installed_version.is_none()
        && p.repository_match.is_some() && (p.executable_path.is_some() || p.installed_path.is_some()) {
        p.status = "versionUnknown".into();
    } else {
        selection::update_status(p);
    }
}

fn same_update_source(a: &Project, b: &Project) -> bool {
    has_repository(a) && same_repository(a, b) && a.channel == b.channel && !a.updates_muted
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
        let reclassify=changes.channel.is_some() || changes.preference.is_some() || changes.management_mode.is_some() || changes.asset_pattern.is_some();
        let local_edit=changes.installed_version.is_some() || changes.installed_path.is_some() || changes.executable_path.is_some();
        let p=self.store.mutate_project(id,local_edit,|p:&mut Project| {
            if let Some(muted)=changes.updates_muted {
                p.updates_muted=muted;
                if muted && p.status=="checking" { p.status="notChecked".into(); }
            }
            let prior_executable = p.executable_path.clone();
            let prior_automatic_version = p.installed_source.as_deref() == Some("executable");
            let version_edited = changes.installed_version.is_some();
            if let Some(mode)=changes.management_mode {
                validate_management_mode(&mode)?;
                if !has_repository(p) && mode == "files" { return Err("本地程序需要关联 GitHub 仓库后才能切换为文件项目".into()); }
                if p.management_mode!=mode {p.selected_asset_id=None;}
                p.management_mode=mode;
            }
            if p.management_mode=="files" {
                if changes.installed_version.as_ref().is_some_and(Option::is_some)||changes.installed_path.as_ref().is_some_and(Option::is_some)||changes.executable_path.as_ref().is_some_and(Option::is_some) {
                    return Err("文件项目不能填写已安装版本或路径".into());
                }
                p.installed_version=None;p.installed_path=None;p.installed_source=None;p.executable_path=None;
            }
            if let Some(backup_enabled)=changes.backup_enabled {
                p.backup_enabled=backup_enabled;
            }
            if p.management_mode=="files" { p.backup_enabled=false; }
            if let Some(channel)=changes.channel { p.channel=channel;p.release=None;p.selected_asset_id=None;p.status="notChecked".into(); }
            if let Some(v)=changes.preference { p.preference=v; }
            validate_choices(&p.channel,&p.preference)?;
            if let Some(pattern)=changes.asset_pattern {
                p.asset_pattern=pattern.map(|s| bounded_text(&s,200,"文件名规则")).transpose()?;
                if p.management_mode=="files" {p.selected_asset_id=None;}
            }
            if let Some(v)=changes.installed_version {
                p.installed_version=v.map(|s| bounded_text(&s,200,"已安装版本")).transpose()?;
                p.installed_source=(p.installed_version.is_some() || (p.repository_match.is_some() && p.executable_path.is_some())).then(|| "manual".into());
            }
            if let Some(path)=changes.installed_path {
                let path=path.map(|s| bounded_text(&s,4096,"已安装路径")).transpose()?;
                if p.installed_path!=path {
                    p.installed_path=path;p.executable_path=None;
                    if p.installed_version.is_some(){p.installed_source=Some("manual".into());}
                }
            }
            if let Some(executable)=changes.executable_path {
                if let Some(path)=executable {
                    let path=bounded_text(&path,4096,"主程序路径")?;
                    let (root,exe)=self.resolve_executable_selection(p,&path)?;
                    let root=root.to_string_lossy().into_owned();
                    if !p.installed_path.as_deref().is_some_and(|old|std::fs::canonicalize(old).ok()==std::fs::canonicalize(&root).ok()) {p.installed_source=Some("manual".into());}
                    p.installed_path=Some(root);p.executable_path=Some(exe.to_string_lossy().into_owned());
                } else {p.executable_path=None;}
            }
            if reclassify { if let Some(mut r)=p.release.take() {
                for a in &mut r.assets { selection::classify_for_mode(a,p); }
                p.selected_asset_id=if p.management_mode=="files" {selection::select_for_project(p,&r.assets,p.selected_asset_id)}else{selection::choose(&r.assets)};
                p.release=Some(r);
            } }
            if let Some(selected)=changes.selected_asset_id {
                if let Some(id)=selected {
                    if !p.release.as_ref().is_some_and(|r| r.assets.iter().any(|a| a.id==id && if p.management_mode=="files"{a.downloadable}else{a.compatible})) {
                        return Err("附件不适用于当前项目或 Windows x64".into());
                    }
                }
                p.selected_asset_id=selected;
            }
            if prior_executable != p.executable_path {
                if p.repository_match.as_deref() == Some("pending") { p.repository_match = Some("unmatched".into()); }
                if prior_automatic_version && !version_edited {
                    p.installed_version = None;
                    p.installed_source = None;
                }
            }
            selection::update_file_status(p,&downloads,&self.data_dir);
            if !has_repository(p) || (p.release.is_some() && p.status != "checking") { update_project_status(p); }
            p.error=None; Ok(())
        })?;
        self.changed(); Ok(p)
    }
    pub(crate) fn unread_updates(&self) -> Result<Vec<Project>,String> {
        let downloads=self.store.downloads()?;
        let mut projects=self.store.projects()?;
        let read=self.store.current_read_tags()?;
        projects.retain_mut(|project| {
            selection::update_file_status(project,&downloads,&self.data_dir);
            project.available_update_tag().is_some_and(|tag| read.get(&project.id).map(String::as_str)!=Some(tag))
        });
        Ok(projects)
    }
    pub(crate) async fn check_updates(self:&Arc<Self>,payload:&Value)->Result<Value,String> {
        let _guard=self.checks.lock().await;
        let allow_auto=match payload.get("allowAutoDownload") { None=>true, Some(Value::Bool(v))=>*v,_=>return Err("自动下载参数无效".into()) };
        let ids:Option<Vec<String>>=match payload.get("projectIds") { None=>None,Some(v)=>Some(super::decode(v.clone())?) };
        if ids.as_ref().is_some_and(|v| v.len()>1000) { return Err("一次最多检查1000个项目".into()); }
        self.verify_installations(true).await?;
        let projects=self.store.projects()?;
        if let Some(ids)=&ids { for id in ids { if !projects.iter().any(|p| &p.id==id) { return Err("要检查的项目不存在".into()); } } }
        let mut checked=0; let mut updates=0; let mut found=std::collections::HashMap::new(); let mut errors=0;
        for p in projects.into_iter().filter(|p| has_repository(p) && !p.updates_muted && ids.as_ref().is_none_or(|v| v.contains(&p.id))) {
            let Some(updated) = self.check_linked_project(&p, false, allow_auto).await? else { continue; };
            checked += 1;
            if updated.status == "error" { errors += 1; }
            if updated.status == "updateAvailable" { updates += 1; }
            if let Some(tag) = updated.available_update_tag() { found.insert(updated.id.clone(), tag.to_owned()); }
        }
        let unread=if found.is_empty() {0} else {self.unread_updates()?.iter().filter(|project|
            found.get(&project.id).map(String::as_str)==project.available_update_tag()).count()};
        Ok(json!({"checked":checked,"updates":updates,"unreadUpdates":unread,"errors":errors}))
    }
    pub(super) async fn check_project_after_repository_change(self:&Arc<Self>, p:&Project) -> Result<(),String> {
        let _guard = self.checks.lock().await;
        self.check_linked_project(p, true, false).await.map(|_| ())
    }

    async fn check_linked_project(self:&Arc<Self>, expected:&Project, first_only:bool, allow_auto:bool) -> Result<Option<Project>,String> {
        let id = &expected.id;
        let Some((mut request, ())) = self.store.mutate_project_if(id, |p| {
            if !same_update_source(p, expected) || (first_only && p.checked_at.is_some()) { return None; }
            p.status = "checking".into();
            p.error = None;
            Some(())
        })? else { return Ok(None); };
        self.changed();
        let allow_auto = allow_auto && !(request.repository_match.is_some() && request.release.is_none());
        let fetched = async {
            let raw = self.github.api(&request.owner, &request.repo, &[], None).await?;
            metadata(&mut request, &raw)?;
            let release = self.github.release(&request).await?;
            let head = self.github.head(&request.owner, &request.repo, &request.default_branch).await.ok();
            let tag_commit = if let Some(r) = &release { self.github.head(&request.owner, &request.repo, &r.tag).await.ok() } else { None };
            Ok::<_, String>((release, head, tag_commit))
        }.await;
        let downloads = self.store.downloads()?;
        let Some((updated, ())) = self.store.mutate_project_if(id, |current| {
            // The same guard applies to successes and failures. `checking` also rejects a switch away and back.
            if current.status != "checking" || !same_update_source(current, &request) { return None; }
            current.checked_at = Some(now());
            match fetched {
                Ok((mut release, head, tag_commit)) => {
                    current.name = request.name; current.description = request.description;
                    current.avatar_url = request.avatar_url; current.default_branch = request.default_branch;
                    if let Some(r) = &mut release { for a in &mut r.assets { selection::classify_for_mode(a, current); } }
                    current.selected_asset_id = release.as_ref().and_then(|r| selection::select_for_project(current, &r.assets, current.selected_asset_id));
                    current.release = release;
                    current.source_ahead = head.as_ref().zip(tag_commit.as_ref()).map(|(a,b)| a != b);
                    current.source_head_sha = head;
                    current.error = None;
                    selection::update_file_status(current, &downloads, &self.data_dir);
                    update_project_status(current);
                }
                Err(error) => { current.status = "error".into(); current.error = Some(error); }
            }
            Some(())
        })? else { return Ok(None); };
        if allow_auto && updated.status == "updateAvailable" && self.store.settings()?.auto_download {
            if let (Some(asset), Some(release)) = (updated.selected_asset_id, updated.release.as_ref()) {
                let exists = self.store.downloads()?.iter().any(|d| d.project_id == updated.id && d.asset_id == asset && d.tag == release.tag);
                if !exists {
                    if let Err(e) = self.queue_auto_download(id, asset) {
                        self.store.mutate_project_if(id, |p| {
                            if !same_update_source(p, &updated) || p.available_update_tag() != Some(release.tag.as_str()) || p.selected_asset_id != Some(asset) { return None; }
                            p.error = Some(format!("自动下载未开始：{e}"));
                            Some(())
                        })?;
                    }
                }
            }
        }
        self.changed();
        Ok(Some(updated))
    }

    pub(crate) fn export_library(&self)->Result<String,String> {
        let s=self.store.settings()?;
        let library=Library {version:1,projects:self.store.projects()?.into_iter().filter(has_repository).map(|p|Subscription{
            repository:format!("{}/{}",p.owner,p.repo),channel:p.channel,preference:p.preference,asset_pattern:p.asset_pattern,backup_enabled:p.backup_enabled&&p.management_mode=="software",updates_muted:p.updates_muted,management_mode:p.management_mode}).collect(),
            repository_notes:self.store.repository_notes()?,
            settings:LibrarySettings{theme:s.theme,color_scheme:Some(s.color_scheme),material:Some(s.material),blur:Some(s.blur),library_layout:Some(s.library_layout),library_sort:(s.library_sort!="manual").then_some(s.library_sort),check_interval_minutes:s.check_interval_minutes,notifications:s.notifications,
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
        if let Some(layout)=library.settings.library_layout {settings.library_layout=layout;}
        if let Some(sort)=library.settings.library_sort {settings.library_sort=sort;}
        settings.notifications=library.settings.notifications;settings.auto_download=library.settings.auto_download;
        settings.max_concurrent_downloads=library.settings.max_concurrent_downloads;settings.close_to_tray=library.settings.close_to_tray;
        if let Some(connections)=library.settings.download_connections {settings.download_connections=connections;}
        if let Some(page)=library.settings.startup_page {settings.startup_page=page;}
        if let Some(tab)=library.settings.startup_github_tab {settings.startup_github_tab=tab;}
        if let Some(mut order)=library.settings.navigation_order {
            upgrade_navigation_order(&mut order);
            settings.navigation_order=order;
        }
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
            p.backup_enabled=input.backup_enabled&&p.management_mode=="software";
            p.updates_muted=input.updates_muted;
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
    #[test] fn library_layout_export_import_and_preserve_legacy_choice() {
        let dir=tempfile::tempdir().unwrap();
        let source=Core::new(dir.path().join("source"),true,Arc::new(|_,_|{})).unwrap();
        let target=Core::new(dir.path().join("target"),true,Arc::new(|_,_|{})).unwrap();
        let mut value=Value::Null;
        for layout in ["two","three","list"] {
            let mut settings=source.store.settings().unwrap(); settings.library_layout=layout.into(); source.store.save_settings(&settings).unwrap();
            let exported=source.export_library().unwrap(); value=serde_json::from_str(&exported).unwrap();
            assert_eq!(value["settings"]["libraryLayout"],layout);
            target.import_library(&exported).unwrap(); assert_eq!(target.store.settings().unwrap().library_layout,layout);
        }
        value["settings"].as_object_mut().unwrap().remove("libraryLayout");
        for layout in ["two","three","list"] {
            let mut settings=target.store.settings().unwrap(); settings.library_layout=layout.into(); target.store.save_settings(&settings).unwrap();
            target.import_library(&value.to_string()).unwrap(); assert_eq!(target.store.settings().unwrap().library_layout,layout);
        }
        let legacy=Core::new(dir.path().join("legacy"),true,Arc::new(|_,_|{})).unwrap(); legacy.import_library(&value.to_string()).unwrap();
        assert_eq!(legacy.store.settings().unwrap().library_layout,"three");
        value["settings"]["libraryLayout"]=json!("grid");
        assert_eq!(target.import_library(&value.to_string()).unwrap_err(),"资源库布局无效");
        assert_eq!(target.store.settings().unwrap().library_layout,"list");
    }
    #[test] fn library_sort_roundtrips_and_legacy_import_keeps_sort_and_navigation_order() {
        let dir=tempfile::tempdir().unwrap();
        let core=Core::new(dir.path().join("data"),true,Arc::new(|_,_|{})).unwrap();
        let mut settings=core.store.settings().unwrap();
        settings.library_sort="nameDesc".into();
        core.store.save_settings(&settings).unwrap();
        let exported=core.export_library().unwrap();
        let mut legacy:Value=serde_json::from_str(&exported).unwrap();
        assert_eq!(legacy["settings"]["librarySort"],"nameDesc");
        settings.library_sort="updated".into();
        core.store.save_settings(&settings).unwrap();
        core.import_library(&exported).unwrap();
        assert_eq!(core.store.settings().unwrap().library_sort,"nameDesc");
        settings.library_sort="updated".into();
        core.store.save_settings(&settings).unwrap();
        legacy["settings"].as_object_mut().unwrap().remove("librarySort");
        legacy["settings"]["navigationOrder"]=json!(["agent","github","library","updates","downloads","home"]);
        core.import_library(&serde_json::to_string(&legacy).unwrap()).unwrap();
        let imported=core.store.settings().unwrap();
        assert_eq!(imported.library_sort,"updated");
        assert_eq!(imported.navigation_order,["agent","github","trainers","library","updates","downloads","home"]);
    }
    #[tokio::test] async fn library_manual_order_is_local_and_survives_database_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let core = Core::new(data.clone(), true, Arc::new(|_, _| {})).unwrap();
        let software = empty_project("fixture".into(), "software".into(), "stable".into(), "auto".into());
        let mut file = empty_project("fixture".into(), "file".into(), "stable".into(), "auto".into());
        file.management_mode = "files".into();
        core.store.put_project(&software).unwrap();
        core.store.put_project(&file).unwrap();
        core.handle("save_settings", json!({"settings":{"librarySort":"manual", "librarySoftwareOrder":[software.id]}})).await.unwrap();
        core.handle("save_settings", json!({"settings":{"libraryFilesOrder":[file.id]}})).await.unwrap();
        let exported = core.export_library().unwrap();
        assert!(!exported.contains(&software.id) && !exported.contains(&file.id));
        let portable: Value = serde_json::from_str(&exported).unwrap();
        for key in ["librarySort", "librarySoftwareOrder", "libraryFilesOrder"] { assert!(portable["settings"].get(key).is_none()); }
        core.import_library(&exported).unwrap();
        assert!(core.handle("save_settings", json!({"settings":{"librarySoftwareOrder":[software.id, software.id]}})).await.is_err());
        assert!(core.handle("save_settings", json!({"settings":{"libraryFilesOrder":["fixture.exe"]}})).await.is_err());
        drop(core);
        let core = Core::new(data, true, Arc::new(|_, _| {})).unwrap();
        let snapshot = core.handle("snapshot", json!({})).await.unwrap();
        assert_eq!(snapshot["settings"]["librarySort"], "manual");
        assert_eq!(snapshot["settings"]["librarySoftwareOrder"], json!([software.id]));
        assert_eq!(snapshot["settings"]["libraryFilesOrder"], json!([file.id]));
    }
    #[test] fn exports_only_subscriptions() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let mut p=empty_project("owner".into(),"repo".into(),"stable".into(),"portable".into());
        p.installed_path=Some(dir.path().join("fixture-app").to_string_lossy().into_owned());
        p.executable_path=Some(dir.path().join("fixture-app").join("fixture.exe").to_string_lossy().into_owned());
        p.installed_version=Some("1.2.0".into());core.store.put_project(&p).unwrap();
        let mut settings=core.store.settings().unwrap();settings.auto_download=true;settings.agent_enabled=true;settings.library_layout="list".into();core.store.save_settings(&settings).unwrap();
        core.set_github_token(Some("private-test-token".into()));let export=core.export_library().unwrap();
        let value:Value=serde_json::from_str(&export).unwrap(); assert_eq!(value["settings"]["libraryLayout"],"list");
        assert!(!export.contains("private-test-token"));assert!(!export.contains("installedPath"));assert!(!export.contains("installedVersion"));
        assert!(!export.contains("executablePath"));assert!(!export.contains("fixture-app"));
        assert!(!export.contains("buildTasks"));assert!(!export.contains("agentPort"));assert!(!export.contains("agentEnabled"));
        assert_eq!(core.import_library(&export).unwrap()["skipped"],1);
        let imported=Core::new(dir.path().join("fresh-import"),true,Arc::new(|_,_|{})).unwrap();
        assert_eq!(imported.import_library(&export).unwrap()["imported"],1);
        assert!(imported.store.settings().unwrap().auto_download);assert!(!imported.store.settings().unwrap().agent_enabled);assert_eq!(imported.store.settings().unwrap().library_layout,"list");
        assert!(!imported.github.token_configured());assert!(imported.store.downloads().unwrap().is_empty());
        let restored=&imported.store.projects().unwrap()[0];assert!(restored.installed_path.is_none());assert!(restored.installed_version.is_none());assert!(restored.executable_path.is_none());
    }
    #[test] fn notes_and_startup_preferences_export_without_proxy_or_credentials() {
        let dir=tempfile::tempdir().unwrap(); let core=Core::new(dir.path().join("source"),true,Arc::new(|_,_|{})).unwrap();
        let note=RepositoryNote { repository:"Owner/Unsubscribed".into(),name:"本地名称".into(),description:"不会发给 GitHub".into(),updated_at:now() };
        core.store.save_repository_note(&note).unwrap();
        let mut settings=core.store.settings().unwrap(); settings.startup_page="github".into(); settings.startup_github_tab="starred".into(); settings.navigation_order.reverse(); settings.download_connections=8; settings.use_external_downloader=true; settings.external_downloader.name="fixture-local-downloader".into();
        settings.library_layout="two".into(); settings.external_downloader.endpoint="http://127.0.0.1:15152/fixture-export-only".into();
        settings.external_downloader.request_template=r#"{"fixtureLink":"{url}"}"#.into();
        settings.proxy=crate::proxy::ProxySettings { mode:"manual".into(),url:"http://127.0.0.1:9".into(),username:"fixture-proxy-user".into() };
        core.store.save_settings(&settings).unwrap(); core.proxy.configure(settings.proxy.clone(),Some("fixture-only-password".into())).unwrap();
        let export=core.export_library().unwrap(); let value:Value=serde_json::from_str(&export).unwrap();
        assert!(value["settings"].get("proxy").is_none()); assert!(!export.contains("fixture-proxy-user")); assert!(!export.contains("fixture-only-password"));
        assert_eq!(value["settings"]["downloadConnections"],8); assert!(value["settings"].get("externalDownloader").is_none()); assert!(value["settings"].get("useExternalDownloader").is_none()); assert!(!export.contains("fixture-local-downloader"));
        assert_eq!(value["settings"]["libraryLayout"],"two"); assert!(!export.contains("fixture-export-only")); assert!(!export.contains("fixtureLink"));
        assert_eq!(value["repositoryNotes"][0]["repository"],"owner/unsubscribed");
        let target=Core::new(dir.path().join("target"),true,Arc::new(|_,_|{})).unwrap();
        let mut target_settings=target.store.settings().unwrap(); target_settings.proxy.mode="direct".into(); target.store.save_settings(&target_settings).unwrap();
        let result=target.import_library(&export).unwrap(); assert_eq!(result["imported"],0); assert_eq!(result["notesImported"],1);
        assert_eq!(target.store.settings().unwrap().startup_page,"github"); assert_eq!(target.store.settings().unwrap().startup_github_tab,"starred");
        assert_eq!(target.store.settings().unwrap().navigation_order,settings.navigation_order); assert_eq!(target.store.settings().unwrap().proxy.mode,"direct");
        assert_eq!(target.store.settings().unwrap().download_connections,8); assert!(!target.store.settings().unwrap().use_external_downloader);
        assert_eq!(target.store.settings().unwrap().library_layout,"two");
        assert_eq!(serde_json::to_value(target.store.settings().unwrap().external_downloader).unwrap(),serde_json::to_value(ExternalDownloader::default()).unwrap());
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
