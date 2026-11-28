//! Direct in-place installs with opt-in full snapshots and durable interruption tracking.
#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
use super::{install, installation_checks::{Identity, Probe}, models::*, windows_probe::{self, InstallerProcess, LaunchError}, Core};
use serde_json::{json, Value};
use std::{collections::{BTreeSet, HashSet}, fs, io::Write, path::{Component, Path, PathBuf}, sync::Arc};

fn io_error(_: impl std::fmt::Display) -> String { "安装文件读写失败，请关闭软件并检查权限和磁盘空间".into() }
pub(super) fn linked(meta: &fs::Metadata) -> bool {
    let value = meta.file_type().is_symlink();
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; return value || meta.file_attributes() & 0x400 != 0; }
    #[cfg(not(windows))] { value }
}
/// Reject links at every lexical ancestor, not merely the canonical leaf.
fn safe_existing(path: &Path, directory: bool) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err("必须选择完整的本地路径，不能包含相对跳转".into());
    }
    #[cfg(windows)] {
        let text = path.to_string_lossy();
        if text.starts_with("\\\\") && !(text.starts_with("\\\\?\\") && text.as_bytes().get(5) == Some(&b':')) {
            return Err("安装目录必须位于本地磁盘，不能使用网络或设备路径".into());
        }
    }
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(io_error)?;
        if linked(&meta) { return Err("路径不能包含符号链接、目录联接或重解析点".into()); }
    }
    let meta = fs::symlink_metadata(path).map_err(io_error)?;
    if (directory && !meta.is_dir()) || (!directory && !meta.is_file()) { return Err("所选路径类型不正确".into()); }
    fs::canonicalize(path).map_err(io_error)
}
fn overlaps(a: &Path, b: &Path) -> bool { a.starts_with(b) || b.starts_with(a) }
fn tree_files(root: &Path) -> Result<(Vec<(PathBuf, bool)>, u64), String> {
    safe_existing(root, true)?;
    let mut result = Vec::new(); let mut total = 0u64; let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for item in fs::read_dir(&directory).map_err(io_error)? {
            let item = item.map_err(io_error)?; let path = item.path(); let meta = fs::symlink_metadata(&path).map_err(io_error)?;
            if linked(&meta) || (!meta.is_dir() && !meta.is_file()) { return Err("软件目录含链接或特殊文件，无法完整备份".into()); }
            let relative = path.strip_prefix(root).map_err(io_error)?.to_owned();
            if result.len() >= 100000 { return Err("软件目录文件数超过安全限制".into()); }
            result.push((relative, meta.is_dir()));
            if meta.is_dir() { pending.push(path); } else { total = total.checked_add(meta.len()).ok_or("软件目录大小超限")?; }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0)); Ok((result, total))
}
fn copy_tree(source: &Path, destination: &Path, overwrite: bool) -> Result<u64, String> {
    let (entries, bytes) = tree_files(source)?; safe_existing(destination, true)?;
    for (relative, directory) in entries {
        let from = source.join(&relative); let to = destination.join(&relative);
        safe_existing(&from, directory)?;
        if directory {
            if to.exists() { safe_existing(&to, true)?; } else {
                safe_existing(to.parent().ok_or("目录没有上层")?, true)?; fs::create_dir(&to).map_err(io_error)?;
            }
        } else {
            safe_existing(to.parent().ok_or("文件没有上层")?, true)?;
            if to.exists() { if !overwrite { return Err("备份目标已存在".into()); } safe_existing(&to, false)?; }
            let mut input = fs::File::open(&from).map_err(io_error)?;
            let mut options = fs::OpenOptions::new(); options.write(true);
            if overwrite { options.create(true).truncate(true); } else { options.create_new(true); }
            let mut output = options.open(&to).map_err(io_error)?;
            std::io::copy(&mut input, &mut output).map_err(io_error)?; output.sync_all().map_err(io_error)?;
            fs::set_permissions(&to, fs::metadata(&from).map_err(io_error)?.permissions()).map_err(io_error)?;
        }
    }
    Ok(bytes)
}
/// Inventory the entire tree before removing anything. Never follow links during deletion.
fn clear_tree(root: &Path) -> Result<(), String> {
    let (mut entries, _) = tree_files(root)?; entries.sort_by_key(|a| std::cmp::Reverse(a.0.components().count()));
    for (relative, directory) in entries {
        let path = root.join(relative); safe_existing(&path, directory)?;
        if directory { fs::remove_dir(&path).map_err(io_error)?; } else { fs::remove_file(&path).map_err(io_error)?; }
    }
    Ok(())
}
fn free_bytes(path: &Path) -> Result<u64, String> {
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        #[link(name="kernel32")] extern "system" { fn GetDiskFreeSpaceExW(path:*const u16, available:*mut u64, total:*mut u64, free:*mut u64)->i32; }
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect(); let mut available=0;
        if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 { return Err("无法读取磁盘剩余空间".into()); }
        Ok(available)
    }
    #[cfg(not(windows))] { let _=path; Ok(u64::MAX) }
}
fn writable_and_idle(root: &Path) -> Result<(), String> {
    let (entries, _) = tree_files(root)?;
    for (relative, directory) in entries {
        if !directory {
            let mut options=fs::OpenOptions::new(); options.read(true).write(true);
            #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(0); }
            options.open(root.join(relative)).map_err(|_| "软件文件正被占用或只读，请关闭软件后重试".to_string())?;
        }
    }
    let probe=root.join(format!(".releasehub-write-{}",uuid::Uuid::new_v4()));
    fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(io_error)?;
    safe_existing(&probe,false)?; fs::remove_file(probe).map_err(io_error)
}
fn inspect_zip(source: &Path) -> Result<(Vec<String>, u64, bool), String> {
    let mut zip=zip::ZipArchive::new(fs::File::open(source).map_err(io_error)?).map_err(|_| "ZIP 文件损坏".to_string())?;
    if zip.is_empty() || zip.len()>10000 { return Err("ZIP 文件为空或条目超过10000个限制".into()); }
    let mut roots=BTreeSet::new(); let mut seen=HashSet::new(); let mut bytes=0u64; let mut files=0; let mut root_file=false;
    for index in 0..zip.len() {
        let entry=zip.by_index(index).map_err(|_|"ZIP 文件目录损坏".to_string())?;
        let path=install::zip_path(entry.name())?;
        if entry.enclosed_name().is_none() { return Err("ZIP 包包含越界路径".into()); }
        if let Some(mode)=entry.unix_mode() { if ![0,0o100000,0o040000].contains(&(mode&0o170000)) { return Err("ZIP 包含链接或特殊文件".into()); } }
        if !seen.insert(path.to_string_lossy().to_lowercase()) { return Err("ZIP 包含重复文件名".into()); }
        bytes=bytes.checked_add(entry.size()).ok_or("ZIP 展开大小超限")?;
        if bytes>8*1024*1024*1024 || entry.size()>super::network::MAX_DOWNLOAD_BYTES { return Err("ZIP 展开内容超过安全大小限制".into()); }
        if !entry.is_dir() { files+=1; if path.components().count()==1 { root_file=true; } }
        let components:Vec<_>=path.components().collect();
        if components.len()>1 || entry.is_dir() { if let Some(Component::Normal(part))=components.first() { roots.insert(part.to_string_lossy().into_owned()); } }
    }
    if files==0 { return Err("ZIP 没有可安装文件".into()); }
    let mut choices=vec![".".into()]; choices.extend(roots); Ok((choices,bytes,root_file))
}
fn snapshot_inventory(root:&Path)->Result<Vec<SnapshotEntry>,String> {
    use sha2::{Digest,Sha256};
    let (files,_)=tree_files(root)?;let mut inventory=Vec::new();
    for (relative,directory) in files {
        let path=root.join(&relative);let bytes=if directory {0}else{fs::metadata(&path).map_err(io_error)?.len()};
        let sha256=if directory {None}else{
            let mut input=fs::File::open(&path).map_err(io_error)?;let mut digest=Sha256::new();
            let mut buffer=[0u8;65536];loop{let count=std::io::Read::read(&mut input,&mut buffer).map_err(io_error)?;if count==0{break;}digest.update(&buffer[..count]);}
            Some(format!("{:x}",digest.finalize()))
        };
        inventory.push(SnapshotEntry{path:relative.to_string_lossy().into_owned(),directory,bytes,sha256});
    }
    Ok(inventory)
}
fn standalone(p:&Project,d:&DownloadRecord)->bool {
    p.owner.eq_ignore_ascii_case("Lake1059") && p.repo.eq_ignore_ascii_case("FFmpegFreeUI") && d.file_name.eq_ignore_ascii_case("FFmpegFreeUI.x64.exe")
}
impl Core {
    fn validate_target(&self,p:&Project,target:&str)->Result<(PathBuf,bool),String> {
        let target=safe_existing(Path::new(target),true)?;
        if target.parent().is_none() || target.file_name().is_none() { return Err("不能选择磁盘根目录".into()); }
        if overlaps(&target,&self.data_dir) { return Err("安装目录不能包含 ReleaseHub 资料库或下载、备份目录".into()); }
        if let Ok(exe)=std::env::current_exe() { if let Some(parent)=exe.parent() { if overlaps(&target,parent) { return Err("不能选择 ReleaseHub 程序目录".into()); } } }
        #[cfg(windows)] for name in ["WINDIR","SystemRoot"] {
            if let Some(path)=std::env::var_os(name) { if let Ok(windows)=fs::canonicalize(path) { if overlaps(&target,&windows) { return Err("不能选择 Windows 系统目录".into()); } } }
        }
        for other in self.store.projects()?.into_iter().filter(|other|other.id!=p.id) {
            if let Some(path)=other.installed_path.and_then(|path|fs::canonicalize(path).ok()) { if overlaps(&target,&path) { return Err("所选目录与另一个已记录软件目录重叠".into()); } }
        }
        let legacy=p.installed_source.as_deref()==Some("managed");
        if let Some(old)=p.installed_path.as_deref() {
            let old=safe_existing(Path::new(old),true)?;
            if legacy {
                if !old.starts_with(self.data_dir.join("apps").join(&p.id)) { return Err("旧受管目录记录不安全".into()); }
                if !tree_files(&target)?.0.is_empty() { return Err("旧受管软件迁移必须选择新的空目录".into()); }
            } else if old!=target { return Err("已有软件必须在记录的原目录更新；请先明确修改安装路径".into()); }
        } else if !tree_files(&target)?.0.is_empty() {
            return Err("未记录软件原路径时只能选择已存在的空目录；更新已有软件请先明确记录原软件路径".into());
        }
        tree_files(&target)?; Ok((target,legacy))
    }
    pub(crate) fn preview_install(&self,id:&str,target:Option<&str>)->Result<InstallPreview,String> {
        let d=self.store.download(id)?; let p=self.store.project(&d.project_id)?; require_software(&p)?;
        if !d.installable { return Err("此附件仅供下载，不能安装".into()); }
        if !["completed","installed"].contains(&d.status.as_str()) { return Err("请先完成下载后再安装".into()); }
        if self.store.journals()?.iter().any(|j|j.backup.project_id==p.id) { return Err("上次安装尚待恢复，请先恢复备份".into()); }
        let source=install::verify_download(&self.data_dir,&d)?;
        let target=target.or(p.installed_path.as_deref()).ok_or("未记录安装路径，请明确选择软件目录")?;
        let (target,legacy)=self.validate_target(&p,target)?;
        let backup_enabled = p.backup_enabled;
        let (backup_path, backup_bytes) = if backup_enabled {
            let (_, mut bytes) = tree_files(&target)?;
            if legacy { bytes=bytes.checked_add(tree_files(Path::new(p.installed_path.as_deref().ok_or("旧目录缺失")?))?.1).ok_or("备份大小超限")?; }
            (Some(self.data_dir.join("backups").join(&p.id).to_string_lossy().into_owned()), bytes)
        } else { (None, 0u64) };
        let (roots,package_bytes,root_file)=if d.kind=="portable" && d.file_name.to_ascii_lowercase().ends_with(".zip") { inspect_zip(&source)? }
            else if d.kind=="installer" || (d.kind=="portable"&&standalone(&p,&d)) { (vec![],d.total_bytes,true) }
            else { return Err("此文件不支持便携安装".into()); };
        let available_bytes=free_bytes(&target)?; let backup_available=free_bytes(&self.data_dir)?;
        let required_package = package_bytes.checked_add(16*1024*1024).ok_or("所需空间超限")?;
        if available_bytes < required_package { return Err("磁盘空间不足以安装".into()); }
        let required = if backup_enabled {
            package_bytes.checked_mul(2).and_then(|package|backup_bytes.checked_add(package)).and_then(|v|v.checked_add(16*1024*1024)).ok_or("所需空间超限")?
        } else { required_package };
        if backup_available < required {
            return Err(if backup_enabled { "磁盘空间不足以完整备份并安装" } else { "磁盘空间不足以暂存安装包" }.into());
        }
        // Never guess a wrapper directory. An explicit root selection is always honored.
        let suggested_root=if !roots.is_empty()&&root_file { Some(".".into()) } else { None };
        Ok(InstallPreview{download_id:d.id,project_id:p.id.clone(),target_path:target.to_string_lossy().into_owned(),
            backup_path,previous_version:p.installed_version,
            next_version:d.tag,backup_bytes,package_bytes,available_bytes,kind:d.kind,requires_root_selection:!roots.is_empty()&&suggested_root.is_none(),
            suggested_root,archive_roots:roots,legacy_managed:legacy,backup_enabled})
    }
    pub(crate) async fn install_confirmed(self:&Arc<Self>,input:InstallInput)->Result<Value,String> {
        if !input.confirmed { return Err("请先确认安装此软件".into()); }
        let d=self.store.download(&input.id)?; let p=self.store.project(&d.project_id)?; require_software(&p)?;
        if !d.installable { return Err("此附件仅供下载，不能安装".into()); }
        {
            let mut installing=self.installing.lock().map_err(|_|"安装状态不可用".to_string())?;
            if !installing.insert(p.id.clone()) { return Err("此项目正在安装或恢复，请稍后再试".into()); }
        }
        let core=self.clone(); let project_id=p.id.clone();
        let result=tokio::task::spawn_blocking(move||core.perform_install(&input)).await.map_err(|_|"安装任务被中断，请重新启动以检查安装状态".to_string()).and_then(|r|r);
        if let Ok(mut state)=self.installing.lock() { state.remove(&project_id); }
        self.changed(); result
    }
    fn backup_folder(&self,project_id:&str)->Result<PathBuf,String> {
        uuid::Uuid::parse_str(project_id).map_err(|_|"项目编号无效".to_string())?;
        let root=self.data_dir.join("backups"); safe_existing(&root,true)?;
        let folder=root.join(project_id); if !folder.exists() { fs::create_dir(&folder).map_err(io_error)?; }
        safe_existing(&folder,true)
    }
    fn validate_backup(&self,record:&BackupRecord)->Result<PathBuf,String> {
        let folder=self.backup_folder(&record.project_id)?; let path=Path::new(&record.path);
        uuid::Uuid::parse_str(path.file_name().and_then(|s|s.to_str()).ok_or("备份目录无效")?).map_err(|_|"备份目录无效".to_string())?;
        let path=safe_existing(path,true)?;
        if path.parent()!=Some(folder.as_path()) { return Err("备份路径不属于此项目".into()); }
        tree_files(&path)?; Ok(path)
    }
    fn write_manifest(&self,j:&InstallJournal)->Result<(),String> {
        let root=self.validate_backup(&j.backup)?; let path=root.join("manifest.json");
        if path.exists() { safe_existing(&path,false)?; }
        let mut file=fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(io_error)?;
        file.write_all(&serde_json::to_vec(j).map_err(io_error)?).map_err(io_error)?;file.sync_all().map_err(io_error)
    }
    /// Rebase only structurally proven internal paths. Never inspect or mutate the old data root.
    fn rebase_backup_record(&self,b:&mut BackupRecord)->Result<Option<PathBuf>,String> {
        let path=Path::new(&b.path);
        if !path.is_absolute()||path.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir)){return Err("备份迁移路径无效".into());}
        uuid::Uuid::parse_str(&b.project_id).map_err(|_|"备份项目编号无效".to_string())?;
        let run=path.file_name().and_then(|s|s.to_str()).ok_or("备份运行编号无效")?;
        uuid::Uuid::parse_str(run).map_err(|_|"备份运行编号无效".to_string())?;
        let project=path.parent().ok_or("备份迁移路径无效")?;let backups=project.parent().ok_or("备份迁移路径无效")?;
        if project.file_name().and_then(|s|s.to_str())!=Some(b.project_id.as_str())||backups.file_name().and_then(|s|s.to_str())!=Some("backups"){return Err("备份迁移路径不属于此项目".into());}
        let old_data=backups.parent().ok_or("备份迁移路径无效")?.to_owned();
        let candidate=self.data_dir.join("backups").join(&b.project_id).join(run);
        if path==candidate{return Ok(None);}
        safe_existing(&candidate,true)?;
        b.path=candidate.to_string_lossy().into_owned();Ok(Some(old_data))
    }
    fn rebase_journal(&self,j:&mut InstallJournal)->Result<(),String> {
        let old_backup=PathBuf::from(&j.backup.path);
        let Some(old_data)=self.rebase_backup_record(&mut j.backup)? else{return Ok(());};
        if Path::new(&j.stage_path)!=old_backup.join("stage"){return Err("安装暂存迁移路径无效".into());}
        j.stage_path=Path::new(&j.backup.path).join("stage").to_string_lossy().into_owned();
        if j.original_project.id!=j.backup.project_id||j.previous_download.project_id!=j.backup.project_id||j.previous_download.id!=j.backup.download_id{return Err("备份迁移记录不一致".into());}
        uuid::Uuid::parse_str(&j.previous_download.id).map_err(|_|"备份下载编号无效".to_string())?;
        let filename=&j.previous_download.file_name;
        if super::downloads::safe_name(filename)!=*filename{return Err("备份下载文件名无效".into());}
        if let Some(recorded)=&j.previous_download.path {
            if Path::new(recorded)!=old_data.join("downloads").join(&j.previous_download.id).join(filename){return Err("备份下载迁移路径无效".into());}
            j.previous_download.path=Some(self.data_dir.join("downloads").join(&j.previous_download.id).join(filename).to_string_lossy().into_owned());
        }
        if j.original_project.installed_source.as_deref()==Some("managed") {
            let recorded=PathBuf::from(j.original_project.installed_path.as_deref().ok_or("旧受管路径记录缺失")?);
            let leaf=recorded.file_name().and_then(|s|s.to_str()).ok_or("旧受管迁移路径无效")?;
            if super::downloads::safe_name(leaf)!=leaf||recorded.parent()!=Some(old_data.join("apps").join(&j.backup.project_id).as_path()){return Err("旧受管迁移路径不属于此项目".into());}
            let candidate=self.data_dir.join("apps").join(&j.backup.project_id).join(leaf);
            safe_existing(&candidate,true)?;
            j.original_project.executable_path=if let Some(exe)=&j.original_project.executable_path {
                let relative=Path::new(exe).strip_prefix(&recorded).map_err(|_|"旧受管主程序迁移路径无效".to_string())?;
                if relative.components().any(|c|!matches!(c,Component::Normal(_))){return Err("旧受管主程序迁移路径无效".into());}
                Some(candidate.join(relative).to_string_lossy().into_owned())
            }else{None};
            j.original_project.installed_path=Some(candidate.to_string_lossy().into_owned());
        }
        // backup.target_path and every manual/external path remain exactly as originally confirmed.
        Ok(())
    }
    pub(super) fn repair_backup_paths(&self)->Result<(),String> {
        for project in self.store.projects()? {
            if let Some(mut backup)=self.store.backup(&project.id)? {
                if self.rebase_backup_record(&mut backup).is_ok_and(|old|old.is_some()) {
                    self.store.put_backup(&backup)?;
                }
            }
        }
        for journal in self.store.journals()? {
            let mut rebased=journal.clone();
            if self.rebase_journal(&mut rebased).is_ok()&&rebased.backup.path!=journal.backup.path {self.store.put_journal(&rebased)?;}
        }
        Ok(())
    }
    fn read_manifest(&self,b:&BackupRecord)->Result<InstallJournal,String> {
        let root=self.validate_backup(b)?; let path=safe_existing(&root.join("manifest.json"),false)?;
        if fs::metadata(&path).map_err(io_error)?.len()>2*1024*1024 { return Err("备份记录过大".into()); }
        let mut j:InstallJournal=serde_json::from_slice(&fs::read(path).map_err(io_error)?).map_err(|_|"备份记录损坏".to_string())?;
        self.rebase_journal(&mut j)?;
        if j.backup.project_id!=b.project_id || j.backup.path!=b.path || j.backup.target_path!=b.target_path { return Err("备份记录不一致".into()); } Ok(j)
    }
    fn remove_snapshot(&self,b:&BackupRecord)->Result<(),String> {
        let path=self.validate_backup(b)?;clear_tree(&path)?;safe_existing(&path,true)?;fs::remove_dir(path).map_err(io_error)
    }
    fn perform_install(self: &Arc<Self>,input:&InstallInput)->Result<Value,String> {
        self.perform_install_checked(input,windows_probe::launch,windows_probe::probe,|id,project,process|self.watch_installer(id,project,process))
    }
    #[cfg(test)]
    fn perform_install_with(&self,input:&InstallInput,launch:impl FnOnce(&Path)->Result<(),String>)->Result<Value,String> {
        self.perform_install_checked(input,|path|launch(path).map(|_|None).map_err(|message|LaunchError{cancelled:false,message}),
            |_,_|Probe::default(),|_,_,_|{})
    }
    fn perform_install_checked(&self,input:&InstallInput,launch:impl FnOnce(&Path)->Result<Option<InstallerProcess>,LaunchError>,
        probe:impl FnOnce(&Project,&[Identity])->Probe, watch:impl FnOnce(String,String,InstallerProcess))->Result<Value,String> {
        let d=self.store.download(&input.id)?;let p=self.store.project(&d.project_id)?;
        require_software(&p)?;
        if !d.installable || !matches!(d.status.as_str(),"completed"|"installed") { return Err("请先完成支持安装的软件包下载".into()); }
        if self.store.journals()?.iter().any(|j|j.backup.project_id==p.id) { return Err("此项目还有待恢复的安装，请先处理备份".into()); }
        if input.previous_version.as_ref().is_some_and(|expected|expected!=&p.installed_version) { return Err("已安装版本已改变，请重新预览并确认".into()); }
        if d.kind=="installer" && !p.backup_enabled {
            let source=install::verify_download(&self.data_dir,&d)?;
            let id=self.launch_checked_installer(&p,&d,&source,None,probe,launch,watch,&mut false)?;
            let message="已打开官方安装向导，完成后将核实版本。此项目未开启备份。";
            return Ok(json!({"kind":"installer","message":message,"path":source.to_string_lossy(),"installed":false,"installationId":id}));
        }
        let target_request=if d.kind=="installer" && input.target_path.trim().is_empty() {
            if p.installed_source.as_deref()==Some("managed") {
                return Err("此项目已开启备份；请先为旧受管软件选择新的空目录，再启动安装向导。".into());
            }
            p.installed_path.as_deref().filter(|path| !path.trim().is_empty())
                .ok_or("此项目已开启备份；请先明确选择软件目录，再启动安装向导。")?
        } else {input.target_path.as_str()};
        let preview=self.preview_install(&input.id,Some(target_request))?;
        let source=install::verify_download(&self.data_dir,&d)?; let target=PathBuf::from(&preview.target_path);
        let root=if preview.archive_roots.is_empty() { None } else {
            let root=input.archive_root.as_deref().ok_or("请明确选择 ZIP 中的软件根目录")?;
            if !preview.archive_roots.iter().any(|choice|choice==root) { return Err("ZIP 软件根目录无效，请重新选择".into()); } Some(root)
        };
        writable_and_idle(&target)?;
        if preview.legacy_managed { writable_and_idle(Path::new(p.installed_path.as_deref().ok_or("旧目录缺失")?))?; }
        let backup_enabled = preview.backup_enabled;
        let folder=if backup_enabled { self.backup_folder(&p.id)?.join(uuid::Uuid::new_v4().to_string()) }
            else { self.data_dir.join("temp").join(uuid::Uuid::new_v4().to_string()) };
        if backup_enabled { fs::create_dir(&folder).map_err(io_error)?; }
        else {
            let temp=self.data_dir.join("temp");
            if !temp.exists() { fs::create_dir(&temp).map_err(io_error)?; }
            safe_existing(&temp,true)?;
        }
        let stage=if backup_enabled { folder.join("stage") } else { folder.clone() };
        fs::create_dir(&stage).map_err(io_error)?;
        let snapshot=if backup_enabled { Some(folder.join("target")) } else { None };
        if let Some(s) = &snapshot { fs::create_dir(s).map_err(io_error)?; }
        let mut j=InstallJournal{backup:BackupRecord{project_id:p.id.clone(),download_id:d.id.clone(),version:p.installed_version.clone(),
            installed_source:p.installed_source.clone(),target_path:preview.target_path.clone(),path:if backup_enabled { folder.to_string_lossy().into_owned() } else { "".into() },created_at:now(),bytes:preview.backup_bytes,
            kind:d.kind.clone(),phase:"preparing".into(),error:None},original_project:p.clone(),previous_download:d.clone(),target_empty:tree_files(&target)?.0.is_empty(),stage_path:stage.to_string_lossy().into_owned(),snapshot_enabled:preview.backup_enabled,snapshot_files:vec![],original_files:vec![]};
        self.store.put_journal(&j)?;
        let prepared=(|| {
            if let Some(s) = &snapshot {
                j.snapshot_files=snapshot_inventory(&target)?;
                copy_tree(&target,s,false)?;
                if snapshot_inventory(s)?!=j.snapshot_files || snapshot_inventory(&target)?!=j.snapshot_files {return Err("原目录在备份过程中改变，请关闭软件并重试".into());}
            }
            if preview.legacy_managed && backup_enabled {
                let original=folder.join("original");fs::create_dir(&original).map_err(io_error)?;
                let old=Path::new(p.installed_path.as_deref().ok_or("旧目录缺失")?);
                j.original_files=snapshot_inventory(old)?;copy_tree(old,&original,false)?;
                if snapshot_inventory(&original)?!=j.original_files||snapshot_inventory(old)?!=j.original_files{return Err("旧受管目录在备份过程中改变，请重试".into());}
            }
            if d.kind=="portable" {
                if d.file_name.to_ascii_lowercase().ends_with(".zip") { install::extract_zip(&source,&stage)?; }
                else { fs::copy(&source,stage.join(&d.file_name)).map_err(io_error)?; }
                if let Some(root)=root { if root!="." { safe_existing(&stage.join(root),true)?; } }
            }
            writable_and_idle(&target)?;
            if backup_enabled {
                if snapshot_inventory(&target)?!=j.snapshot_files{return Err("原目录在安装前已改变，请重新备份".into());}
                self.write_manifest(&j)?;
            }
            Ok::<_,String>(())
        })();
        if let Err(error)=prepared {
            self.store.remove_journal(&p.id)?;
            if j.snapshot_enabled { let _=self.remove_snapshot(&j.backup); }
            else { self.cleanup_unbacked_stage(&j); }
            return Err(error);
        }
        j.backup.phase="installing".into();self.store.put_journal(&j)?;
        let transitioned=self.store.mutate("downloads",&d.id,|record:&mut DownloadRecord|{record.status="installing".into();record.error=None;record.updated_at=now();Ok(())})?;
        self.download_event(&transitioned);
        let old=self.store.backup(&p.id)?;
        let mut wizard_open=false;
        let mut installation_id=None;
        let installed=(|| {
            if d.kind=="installer" {
                installation_id=Some(self.launch_checked_installer(&p,&d,&source,Some(&j),probe,launch,watch,&mut wizard_open)?);
                return Ok::<_,String>(());
            } else {
                if preview.legacy_managed {
                    let original=if j.snapshot_enabled { folder.join("original") }
                        else { PathBuf::from(p.installed_path.as_deref().ok_or("旧目录缺失")?) };
                    copy_tree(&original,&target,true)?;
                }
                let selected=match root { Some(root) if root!="."=>stage.join(root),_=>stage.clone() };
                // Overlay only package members: package names win, absent configuration files survive.
                copy_tree(&selected,&target,true)?;
            }
            let mut project=self.store.project(&p.id)?;require_software(&project)?;
            let mut record=self.store.download(&d.id)?;
            if d.kind=="portable" {
                project.installed_version=Some(d.tag.clone());project.installed_path=Some(preview.target_path.clone());project.installed_source=Some("manual".into());
                if preview.legacy_managed { project.executable_path=p.executable_path.as_deref().and_then(|exe|Path::new(exe).strip_prefix(Path::new(p.installed_path.as_deref()?)).ok()).map(|relative|target.join(relative).to_string_lossy().into_owned()); }
                project.error=None;super::selection::update_status(&mut project);record.status="installed".into();
            } else { record.status="completed".into(); }
            record.error=None;record.updated_at=now();j.backup.phase="ready".into();
            if j.snapshot_enabled { self.write_manifest(&j)?; }
            self.store.finish_install(&project,&record,&j)?;self.download_event(&record);Ok::<_,String>(())
        })();
        if let Err(error)=installed {
            if wizard_open { return Err(format!("安装向导已经打开，但本机记录暂未保存，请先完成向导，再重新打开追新：{error}")); }
            if !j.snapshot_enabled {
                let message=format!("安装失败，未开启备份，目录可能已部分更新。请检查文件后重试：{error}");
                self.finish_unbacked_interruption(&j,&message)?;
                return Err(message);
            }
            j.backup.phase="restoring".into();j.backup.error=Some(error.clone());self.store.put_journal(&j)?;
            match self.restore_journal(&mut j) {
                Ok(())=>{if j.snapshot_enabled { let _=self.remove_snapshot(&j.backup); } return Err(format!("安装失败，已恢复原目录和版本：{error}"));}
                Err(recovery)=>return Err(format!("安装失败，完整备份保留，需恢复后继续：{recovery}")),
            }
        }
        // The previous snapshot is retired only after the new metadata and backup are durably published.
        if j.snapshot_enabled {
            if let Some(old)=old { let _=self.remove_snapshot(&old); }
        }
        let _=clear_tree(&stage);let _=fs::remove_dir(&stage);
        if d.kind=="installer" {
            let message="已备份软件文件并打开官方安装向导。完成后将核实版本；文件备份不包含注册表和系统安装状态。";
            Ok(json!({"kind":"installer","message":message,"path":source.to_string_lossy(),"installed":false,"installationId":installation_id}))
        } else {
            let message=if backup_enabled { "已备份并更新原目录。" } else { "已更新原目录。此项目未开启备份。" };
            Ok(json!({"kind":"portable","message":message,"path":preview.target_path,"installed":true}))
        }
    }
    fn launch_checked_installer(&self,p:&Project,d:&DownloadRecord,source:&Path,journal:Option<&InstallJournal>,
        probe:impl FnOnce(&Project,&[Identity])->Probe,launch:impl FnOnce(&Path)->Result<Option<InstallerProcess>,LaunchError>,
        watch:impl FnOnce(String,String,InstallerProcess),wizard_open:&mut bool)->Result<String,String> {
        let ready=journal.map(|j|{let mut j=j.clone();j.backup.phase="ready".into();j});
        if let Some(j)=&ready { self.write_manifest(j)?; }
        let binding=self.store.install_binding(&p.id)?;
        let baseline=probe(p,binding.as_ref().map_or(&[][..],|b|b.identities.as_slice()));
        let task=self.prepare_installation_check(p,d,baseline);
        self.store.begin_installation_check(&task,journal)?;
        let id=task.check.id;
        let process=match launch(source) {
            Ok(process)=>process,
            Err(error)=>{self.store.fail_installer_launch(&id,error.cancelled,&error.message)?;return Err(error.message);}
        };
        *wizard_open=true;
        let download=self.store.finish_installer_launch(&id,ready.as_ref(),process.is_some(),false)?;
        if ready.is_some() { self.download_event(&download); }
        if let Some(process)=process { watch(id.clone(),p.id.clone(),process); }
        Ok(id)
    }
    // Delete only this installation's internal staging directory, never its target or old data root.
    fn cleanup_unbacked_stage(&self,j:&InstallJournal) {
        let stage=Path::new(&j.stage_path);
        let Some(run)=stage.file_name().and_then(|s|s.to_str()) else {return;};
        if uuid::Uuid::parse_str(run).is_err() || stage.parent().and_then(Path::file_name).and_then(|s|s.to_str())!=Some("temp") {return;}
        let candidate=self.data_dir.join("temp").join(run);
        if safe_existing(&candidate,true).is_ok() && clear_tree(&candidate).is_ok() {let _=fs::remove_dir(candidate);}
    }
    fn finish_unbacked_interruption(&self,j:&InstallJournal,message:&str)->Result<(),String> {
        let mut project=self.store.project(&j.backup.project_id)?;
        // The files may already have changed. Do not present the previous version as known installed.
        project.installed_version=None;project.status="error".into();project.error=Some(message.into());
        let mut download=self.store.optional_download(&j.previous_download.id)?;
        if let Some(record)=download.as_mut() {
            record.status="completed".into();record.error=Some(message.into());record.updated_at=now();
        }
        self.store.finish_restore(&project,download.as_ref(),&j.backup.project_id)?;
        self.cleanup_unbacked_stage(j);
        if let Some(record)=download {self.download_event(&record);}
        Ok(())
    }
    fn restore_journal(&self,j:&mut InstallJournal)->Result<(),String> {
        if !j.snapshot_enabled { return Err("该项目未开启备份，无法执行自动恢复".into()); }
        let result=(|| {
            let root=self.validate_backup(&j.backup)?;
            let target=safe_existing(Path::new(&j.backup.target_path),true)?;
            if overlaps(&target,&self.data_dir) { return Err("恢复目标与资料库重叠".into()); }
            // Reapply all current path restrictions, but recovery may return to a former managed source.
            let mut validation=j.original_project.clone();
            validation.installed_source=Some("manual".into());
            validation.installed_path=Some(j.backup.target_path.clone());
            self.validate_target(&validation,&j.backup.target_path)?;
            if j.snapshot_enabled {
                let snapshot=root.join("target");
                if snapshot_inventory(&snapshot)?!=j.snapshot_files{return Err("完整备份缺失或内容已改变；为保护当前目录，已拒绝恢复".into());}
            }
            writable_and_idle(&target)?;
            if j.snapshot_enabled {
                if j.original_project.installed_source.as_deref()==Some("managed") {
                    let original=root.join("original");
                    if snapshot_inventory(&original)?!=j.original_files {return Err("旧受管完整备份缺失或内容已改变".into());}
                    let old=safe_existing(Path::new(j.original_project.installed_path.as_deref().ok_or("旧目录记录缺失")?),true)?;
                    let managed_root=safe_existing(&self.data_dir.join("apps").join(&j.backup.project_id),true)?;
                    if old.parent()!=Some(managed_root.as_path()){return Err("旧受管恢复路径不安全".into());}
                    writable_and_idle(&old)?;
                }
            }
            clear_tree(&target)?;
            if j.snapshot_enabled {
                copy_tree(&root.join("target"),&target,false)?;
                if j.original_project.installed_source.as_deref()==Some("managed") {
                    let old=Path::new(j.original_project.installed_path.as_deref().ok_or("旧目录记录缺失")?);
                    clear_tree(old)?;copy_tree(&root.join("original"),old,false)?;
                }
            }
            let mut project=self.store.project(&j.backup.project_id)?;
            project.installed_version=j.original_project.installed_version.clone();
            project.installed_path=j.original_project.installed_path.clone();
            project.installed_source=j.original_project.installed_source.clone();
            project.executable_path=j.original_project.executable_path.clone();
            project.error=None;
            super::library::update_project_status(&mut project);
            let mut download=self.store.optional_download(&j.previous_download.id)?;
            // A removed download is independent of the backup and must never be recreated by restore.
            if let Some(download)=download.as_mut() { download.status=j.previous_download.status.clone();download.error=Some("已恢复安装前的文件与版本".into());download.updated_at=now(); }
            self.store.finish_restore(&project,download.as_ref(),&j.backup.project_id)?;
            if let Some(download)=download { self.download_event(&download); }
            Ok::<_,String>(())
        })();
        if let Err(error)=&result { j.backup.phase="recoveryRequired".into();j.backup.error=Some(error.clone());self.store.put_journal(j)?; }
        result
    }
    pub(super) fn recover_installs(&self)->Result<(),String> {
        for mut journal in self.store.journals()? {
            if journal.backup.phase=="preparing" {
                // A preparation crash never touched the target; retain the previous ready backup.
                self.store.remove_journal(&journal.backup.project_id)?;
                if journal.snapshot_enabled {let _=self.remove_snapshot(&journal.backup);}
                else {self.cleanup_unbacked_stage(&journal);}
            } else if journal.backup.kind=="installer" && journal.backup.phase=="installing" && journal.snapshot_enabled
                && self.store.active_installation_records(Some(&journal.backup.project_id))?.iter().any(|t|t.check.download_id==journal.backup.download_id && !t.launched) {
                // The external wizard may still be running. Do not restore files beneath it.
                if let Some(task)=self.store.active_installation_records(Some(&journal.backup.project_id))?.into_iter().find(|t|t.check.download_id==journal.backup.download_id && !t.launched) {
                    let old=self.store.backup(&journal.backup.project_id)?;
                    journal.backup.phase="ready".into();
                    self.store.finish_installer_launch(&task.check.id,Some(&journal),false,true)?;
                    if let Some(old)=old.filter(|old|old.path!=journal.backup.path) { let _=self.remove_snapshot(&old); }
                    let stage=Path::new(&journal.stage_path);
                    if Path::new(&journal.backup.path).join("stage")==stage && self.validate_backup(&journal.backup).is_ok() && safe_existing(stage,true).is_ok() {
                        let _=clear_tree(stage);let _=fs::remove_dir(stage);
                    }
                }
            } else if journal.snapshot_enabled {
                if self.restore_journal(&mut journal).is_ok() {
                    if self.store.backup(&journal.backup.project_id)?.is_none_or(|b|b.path!=journal.backup.path){let _=self.remove_snapshot(&journal.backup);}
                }
            } else {
                self.finish_unbacked_interruption(&journal,"安装被中断，未开启备份，目录可能已部分更新。请检查文件后重试。")?;
            }
        }
        Ok(())
    }
    pub(crate) fn get_backup(&self,project_id:&str)->Result<Option<BackupRecord>,String> {
        self.store.project(project_id)?;
        if let Some(j)=self.store.journals()?.into_iter().find(|j|j.backup.project_id==project_id&&j.backup.phase!="preparing"&&j.snapshot_enabled) { return Ok(Some(j.backup)); }
        self.store.backup(project_id)
    }
    pub(crate) fn restore_backup(&self,project_id:&str)->Result<Project,String> {
        let mut state=self.installing.lock().map_err(|_|"安装状态不可用".to_string())?;
        if state.contains(project_id) { return Err("正在安装或恢复，请稍后再试".into()); }state.insert(project_id.into());drop(state);
        let result=(|| {
            let pending=self.store.journals()?.into_iter().find(|j|j.backup.project_id==project_id);
            let mut journal=if let Some(j)=pending { if j.backup.phase=="preparing" {return Err("备份尚未准备完成".into());}j }
                else { let backup=self.store.backup(project_id)?.ok_or("此项目没有可恢复备份")?;self.read_manifest(&backup)? };
            journal.backup.phase="restoring".into();self.store.put_journal(&journal)?;self.restore_journal(&mut journal)?;
            self.store.project(project_id)
        })();
        if let Ok(mut state)=self.installing.lock(){state.remove(project_id);}self.changed();result
    }
    pub(crate) fn delete_backup(&self,project_id:&str)->Result<(),String> {
        let state=self.installing.lock().map_err(|_|"安装状态不可用".to_string())?;
        if state.contains(project_id)||self.store.journals()?.iter().any(|j|j.backup.project_id==project_id){return Err("安装或恢复未结束，不能删除备份".into());}
        self.store.project(project_id)?;
        if let Some(backup)=self.store.backup(project_id)? { self.remove_snapshot(&backup)?;self.store.remove_backup(project_id)?; }
        self.changed();Ok(())
    }
    pub(crate) fn delete_download(&self,id:&str)->Result<DownloadRecord,String> {
        let installing=self.installing.lock().map_err(|_|"安装状态不可用".to_string())?;
        let _queue=self.download_queue.lock().map_err(|_|"下载队列不可用".to_string())?;
        let d=self.store.download(id)?;
        if installing.contains(&d.project_id)||self.store.journals()?.iter().any(|j|j.backup.project_id==d.project_id) {
            return Err("安装或恢复未结束，不能删除下载文件".into());
        }
        if self.active_downloads.lock().map_err(|_|"下载队列不可用".to_string())?.contains_key(id)||["queued","downloading","installing"].contains(&d.status.as_str()) {
            return Err("请先取消下载并等待任务结束".into());
        }
        uuid::Uuid::parse_str(id).map_err(|_|"下载记录编号无效".to_string())?;
        if super::downloads::safe_name(&d.file_name)!=d.file_name {return Err("下载文件名无效".into());}
        let root=safe_existing(&self.data_dir.join("downloads"),true)?;
        let folder=root.join(id);let package=folder.join(&d.file_name);
        if d.path.as_deref().is_some_and(|path|Path::new(path)!=package) {return Err("下载记录路径不属于受管目录".into());}
        if folder.exists() {
            safe_existing(&folder,true)?;
            // Only the known artifact and its interrupted partial may be deleted; never arbitrary siblings.
            for path in [package,folder.join(format!("{}.part",d.file_name))] {
                match fs::symlink_metadata(&path) {
                    Ok(_)=>{safe_existing(&path,false)?;fs::remove_file(&path).map_err(io_error)?;},
                    Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(io_error(e)),
                }
            }
            if fs::read_dir(&folder).map_err(io_error)?.next().is_none(){safe_existing(&folder,true)?;fs::remove_dir(&folder).map_err(io_error)?;}
        }
        self.store.remove_download(id)?;
        let mut record=d;record.status="deleted".into();record.path=None;record.received_bytes=0;record.sha256=None;record.verified=false;record.error=None;record.updated_at=now();
        self.download_event(&record);self.changed();Ok(record)
    }
    pub fn project_executable(&self,project_id:&str)->Result<PathBuf,String> {
        if self.installing.lock().map_err(|_|"安装状态不可用".to_string())?.contains(project_id)||self.store.journals()?.iter().any(|j|j.backup.project_id==project_id){return Err("正在安装或等待恢复，不能启动软件".into());}
        let p=self.store.project(project_id)?;require_software(&p)?;
        let exe=p.executable_path.as_deref().ok_or("请先选择软件主程序")?;self.validate_executable(&p,exe)
    }
    pub(super) fn validate_executable(&self,p:&Project,path:&str)->Result<PathBuf,String> {
        require_software(p)?;let root=safe_existing(Path::new(p.installed_path.as_deref().ok_or("请先记录软件安装目录")?),true)?;
        let exe=safe_existing(Path::new(path),false)?;
        if !exe.starts_with(&root)||exe.extension().and_then(|x|x.to_str()).is_none_or(|x|!x.eq_ignore_ascii_case("exe")) {return Err("主程序必须是安装目录内的 EXE 文件".into());}
        let data=fs::canonicalize(&self.data_dir).map_err(io_error)?;
        if overlaps(&root,&data)&&p.installed_source.as_deref()!=Some("managed") {return Err("软件主程序目录不安全".into());}Ok(exe)
    }
    pub(super) fn resolve_executable_selection(&self,p:&Project,path:&str)->Result<(PathBuf,PathBuf),String> {
        require_software(p)?;
        let exe=safe_existing(Path::new(path),false)?;
        if exe.extension().and_then(|x|x.to_str()).is_none_or(|x|!x.eq_ignore_ascii_case("exe")) {return Err("请选择本机 EXE 启动程序".into());}
        // Preserve an already-recorded encompassing install root (for example an app with bin/app.exe).
        if let Some(root)=p.installed_path.as_deref().and_then(|root|safe_existing(Path::new(root),true).ok()).filter(|root|exe.starts_with(root)) {
            self.validate_executable(p,path)?;
            return Ok((root,exe));
        }
        let root=exe.parent().ok_or("启动程序目录无效")?.to_owned();
        let data=fs::canonicalize(&self.data_dir).map_err(io_error)?;
        if overlaps(&root,&data) {return Err("请选择独立的软件目录，不能使用追新数据目录或其上层目录".into());}
        Ok((root,exe))
    }
    pub fn preview_project_executable(&self,project_id:&str,path:&str)->Result<Value,String> {
        let project=self.store.project(project_id)?;
        let (root,exe)=self.resolve_executable_selection(&project,path)?;
        Ok(json!({"installedPath":root.to_string_lossy(),"executablePath":exe.to_string_lossy()}))
    }
    pub fn save_project_executable(&self,project_id:&str,path:&str)->Result<Value,String> {
        let changes=serde_json::from_value(json!({"executablePath":path})).map_err(|_|"主程序路径格式无效".to_string())?;
        super::output(self.update_project(project_id,changes)?)
    }
}
