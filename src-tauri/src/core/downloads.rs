use super::{models::*, network, Core};
use super::download_transfer::{self, TransferOptions};
use std::{path::{Path,PathBuf}, sync::{Arc,atomic::{AtomicBool,Ordering}}, time::Duration};

pub fn safe_name(value:&str)->String {
    let mut name:String=value.chars().map(|c|if c.is_control()||"/\\:*?\"<>|".contains(c){'_'}else{c}).take(180).collect();
    name=name.trim_matches([' ','.']).to_owned();
    if name.is_empty(){name="download.bin".into();}
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.as_str()){name.insert(0,'_');}
    name
}
pub fn expected_sha(value:Option<&str>)->Result<Option<String>,String> {
    let Some(value)=value else{return Ok(None);};
    let Some(hash)=value.strip_prefix("sha256:") else{return Ok(None);};
    if hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err("上游 SHA-256 校验值格式无效".into());}
    Ok(Some(hash.to_ascii_lowercase()))
}
pub fn directory(data:&Path,id:&str)->Result<PathBuf,String> {
    uuid::Uuid::parse_str(id).map_err(|_|"下载记录编号无效".to_string())?;
    let root=data.join("downloads");
    let resolved=std::fs::canonicalize(&root).map_err(|_|"无法读取下载目录".to_string())?;
    if !resolved.starts_with(data)||download_transfer::linked(&std::fs::symlink_metadata(&root).map_err(|_|"无法读取下载目录".to_string())?){return Err("下载目录不能链接到外部位置".into());}
    let folder=root.join(id);
    match std::fs::symlink_metadata(&folder){
        Ok(meta)=>{if !meta.is_dir()||download_transfer::linked(&meta)||!std::fs::canonicalize(&folder).map_err(|_|"下载目录无效".to_string())?.starts_with(&resolved){return Err("下载记录目录不安全".into());}},
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{std::fs::create_dir(&folder).map_err(|_|"无法创建下载目录".to_string())?;},
        Err(_)=>return Err("无法读取下载目录".into()),
    }
    Ok(folder)
}
pub(crate) fn local_file_exists(data:&Path,d:&DownloadRecord)->bool {
    if uuid::Uuid::parse_str(&d.id).is_err()||safe_name(&d.file_name)!=d.file_name {return false;}
    let root=data.join("downloads");let folder=root.join(&d.id);let expected=folder.join(&d.file_name);
    if d.path.as_deref().map(Path::new)!=Some(expected.as_path()) {return false;}
    for path in [&root,&folder,&expected] {
        let Ok(meta)=std::fs::symlink_metadata(path) else{return false;};
        if meta.file_type().is_symlink(){return false;}
        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return false;}}
    }
    expected.is_file()&&std::fs::canonicalize(&expected).is_ok_and(|p|p.parent()==Some(folder.as_path()))
}
impl Core {
    pub(crate) fn queue_download(self:&Arc<Self>,project_id:&str,asset_id:u64)->Result<DownloadRecord,String>{
        self.enqueue_project_asset(project_id,asset_id,false)?.ok_or_else(||"未加入下载队列".into())
    }
    pub(crate) fn queue_auto_download(self:&Arc<Self>,project_id:&str,asset_id:u64)->Result<(),String>{
        self.enqueue_project_asset(project_id,asset_id,true).map(|_| ())
    }
    fn enqueue_project_asset(self:&Arc<Self>,project_id:&str,asset_id:u64,automatic:bool)->Result<Option<DownloadRecord>,String>{
        let queued={
            let _guard=self.download_queue.lock().map_err(|_|"下载队列不可用".to_string())?;
            let p=self.store.project(project_id)?;
            if automatic && p.updates_muted {return Ok(None);}
            let release=p.release.as_ref().ok_or("请先检查项目发行版本")?;
            let a=release.assets.iter().find(|a|a.id==asset_id).ok_or("找不到此发行附件")?;
            if !a.downloadable{return Err(format!("无法下载：{}",a.reason));}
            network::trusted_download_url(&a.url)?;
            if a.size==0||a.size>network::MAX_DOWNLOAD_BYTES{return Err("发行附件大小无效或超过4GB限制".into());}
            let digest=expected_sha(a.digest.as_deref())?;
            if let Some(existing)=self.store.downloads()?.into_iter().find(|d|d.project_id==p.id&&d.asset_id==asset_id&&d.tag==release.tag
                &&(["queued","downloading","external","installing"].contains(&d.status.as_str())
                    ||(["completed","installed"].contains(&d.status.as_str())&&local_file_exists(&self.data_dir,d)))){return Ok(Some(existing));}
            let time=now();let record=DownloadRecord{id:uuid::Uuid::new_v4().to_string(),project_id:p.id,project_name:p.name,
                asset_id,file_name:safe_name(&a.name),tag:release.tag.clone(),url:a.url.clone(),kind:a.kind.clone(),
                installable:p.management_mode=="software"&&a.installable,
                total_bytes:a.size,received_bytes:0,status:"queued".into(),path:None,sha256:None,verified:false,error:None,created_at:time.clone(),updated_at:time};
            self.store.set_digest(&record.id,digest.as_deref())?;self.store.put_download(&record)?;record
        };
        self.download_event(&queued);self.changed();self.pump_downloads();Ok(Some(queued))
    }
    pub(crate) fn retry_download(self:&Arc<Self>,id:&str)->Result<DownloadRecord,String>{
        let record={
            let _guard=self.download_queue.lock().map_err(|_|"下载队列不可用".to_string())?;
            if self.active_downloads.lock().map_err(|_|"下载队列不可用".to_string())?.contains_key(id){return Err("下载仍在停止，请稍后重试".into());}
            let previous=self.store.download(id)?;
            let files=self.store.project(&previous.project_id)?.management_mode=="files";
            self.store.mutate("downloads",id,|d:&mut DownloadRecord|{
                let missing=["completed","installed"].contains(&d.status.as_str())&&!local_file_exists(&self.data_dir,d);
                if !["failed","cancelled","deleted"].contains(&d.status.as_str())&&!missing{return Err("只能重试失败、已取消或文件缺失的下载；已有文件不会覆盖".into());}
                network::trusted_download_url(&d.url)?;
                if d.total_bytes==0||d.total_bytes>network::MAX_DOWNLOAD_BYTES{return Err("发行附件大小无效或超过4GB限制".into());}
                if files {d.installable=false;}
                d.status="queued".into();d.received_bytes=0;d.sha256=None;d.verified=false;d.path=None;d.error=None;d.updated_at=now();Ok(())
            })?
        };
        self.download_event(&record);self.changed();self.pump_downloads();Ok(record)
    }
    pub(crate) fn cancel_download(self:&Arc<Self>,id:&str)->Result<(),String>{
        // Set the durable state first; a worker checks cancellation again before publishing a complete file.
        let record=self.store.mutate("downloads",id,|d:&mut DownloadRecord|{
            if !["queued","downloading"].contains(&d.status.as_str()){return Err("此下载已结束，无法取消".into());}
            d.status="cancelled".into();d.error=Some("下载已取消，可重试".into());d.updated_at=now();Ok(())
        })?;
        if let Some(flag)=self.active_downloads.lock().map_err(|_|"下载队列不可用".to_string())?.get(id){flag.store(true,Ordering::Release);}
        self.download_event(&record);self.changed();self.pump_downloads();Ok(())
    }
    pub(crate) fn pump_downloads(self:&Arc<Self>){
        let Ok(settings)=self.store.settings()else{return;};let Ok(mut records)=self.store.downloads()else{return;};records.reverse();
        let Ok(mut active)=self.active_downloads.lock()else{return;};
        for record in records.into_iter().filter(|d|d.status=="queued"){
            if active.len()>=settings.max_concurrent_downloads{break;}
            if active.contains_key(&record.id){continue;}
            let cancel=Arc::new(AtomicBool::new(false));active.insert(record.id.clone(),cancel.clone());
            let core=self.clone();
            tokio::spawn(async move{
                let result=core.download_worker(&record.id,cancel.clone()).await;
                if let Err(error)=result{
                    if let Ok(d)=core.store.mutate("downloads",&record.id,|d:&mut DownloadRecord|{
                        if d.status=="cancelled"||cancel.load(Ordering::Acquire){d.status="cancelled".into();d.error=Some("下载已取消，可重试".into());}
                        else{d.status="failed".into();d.error=Some(error);}d.updated_at=now();Ok(())
                    }){core.download_event(&d);}
                }
                if let Ok(mut active)=core.active_downloads.lock(){active.remove(&record.id);}
                core.changed();core.pump_downloads();
            });
        }
    }
    async fn download_worker(&self,id:&str,cancel:Arc<AtomicBool>)->Result<(),String>{
        let mut record=self.store.mutate("downloads",id,|d:&mut DownloadRecord|{
            if d.status!="queued"{return Err("下载已取消".into());}d.status="downloading".into();d.updated_at=now();Ok(())
        })?;
        self.download_event(&record);
        let folder=directory(&self.data_dir,id)?;
        if safe_name(&record.file_name)!=record.file_name{return Err("下载文件名无效".into());}
        let partial=folder.join(format!("{}.part",record.file_name));let destination=folder.join(&record.file_name);
        download_transfer::prepare_paths(&partial,&destination).await?;
        let result=self.stream_download(&mut record,&partial,&destination,&cancel).await;
        if result.is_err(){if let Err(cleanup)=download_transfer::remove_partial(&partial).await{return Err(format!("{}；{cleanup}",result.err().unwrap()));}}
        result
    }
    async fn stream_download(&self,record:&mut DownloadRecord,partial:&Path,destination:&Path,cancel:&AtomicBool)->Result<(),String>{
        if cancel.load(Ordering::Acquire){return Err("下载已取消".into());}
        let url=network::trusted_download_url(&record.url)?;
        // This client intentionally has no API token, default authorization header, or cookie store.
        let client=self.proxy.client_builder()?.user_agent(concat!("ReleaseHub/", env!("CARGO_PKG_VERSION"), " artifact-download"))
            .connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(3600))
            .redirect(reqwest::redirect::Policy::custom(|attempt|{
                if attempt.previous().len()>=5||network::trusted_download_url(attempt.url().as_str()).is_err(){attempt.error("不可信下载跳转")}else{attempt.follow()}
            })).build().map_err(|_|"下载服务初始化失败".to_string())?;
        let upstream=self.store.digest(&record.id)?;
        let connections=self.store.settings()?.download_connections;
        let outcome=download_transfer::transfer(&client,TransferOptions{
            url:url.as_str(),expected_size:record.total_bytes,connections,expected_sha256:upstream.as_deref(),
        },partial,cancel,&|url|network::trusted_download_url(url.as_str()).map(|_|()),&mut |progress|{
            let current=self.store.mutate("downloads",&record.id,|d:&mut DownloadRecord|{
                if d.status=="cancelled"{return Err("下载已取消".into());}d.received_bytes=progress;d.updated_at=now();Ok(())
            })?;self.download_event(&current);Ok(())
        }).await?;
        record.received_bytes=outcome.received_bytes;
        let digest=outcome.sha256;
        directory(&self.data_dir,&record.id)?;
        if cancel.load(Ordering::Acquire){return Err("下载已取消".into());}
        download_transfer::publish_partial(partial,destination).await?;
        let completed=self.store.mutate("downloads",&record.id,|d:&mut DownloadRecord|{
            if d.status=="cancelled"||cancel.load(Ordering::Acquire){return Err("下载已取消".into());}
            d.status="completed".into();d.received_bytes=record.received_bytes;d.path=Some(destination.to_string_lossy().into_owned());
            d.sha256=Some(digest);d.verified=upstream.is_some();d.error=None;d.updated_at=now();Ok(())
        });
        match completed{Ok(d)=>{let _=tokio::fs::remove_file(partial).await;self.download_event(&d);Ok(())},Err(e)=>{let _=tokio::fs::remove_file(destination).await;Err(e)}}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn file_names_cannot_escape(){assert_eq!(safe_name("../CON.exe"),"_CON.exe");assert!(!safe_name("a\\b:evil.zip").contains(['\\',':','/']));}
    #[test]fn publisher_digest_is_separate_from_computed_hash(){assert_eq!(expected_sha(None).unwrap(),None);assert!(expected_sha(Some("sha256:bad")).is_err());assert!(expected_sha(Some(&format!("sha256:{}","a".repeat(64)))).unwrap().is_some());}
    fn paused_queue()->(tempfile::TempDir,Arc<Core>,Project){
        let dir=tempfile::tempdir().unwrap();let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();
        let mut settings=core.store.settings().unwrap();settings.max_concurrent_downloads=1;core.store.save_settings(&settings).unwrap();
        // Occupy the only slot: these lifecycle tests must never start a network request.
        core.active_downloads.lock().unwrap().insert("held-test-slot".into(),Arc::new(AtomicBool::new(false)));
        let mut project=test_project("owner","app");
        project.release=Some(Release{id:1,tag:"v1.0.0".into(),name:"test".into(),body:String::new(),published_at:now(),
            url:"https://github.com/owner/app/releases/tag/v1.0.0".into(),prerelease:false,
            assets:vec![ReleaseAsset{id:7,name:"app-win-x64.zip".into(),size:10,
                url:"https://github.com/owner/app/releases/download/v1.0.0/app-win-x64.zip".into(),content_type:"application/zip".into(),
                digest:Some(format!("sha256:{}","a".repeat(64))),kind:"portable".into(),downloadable:true,installable:true,compatible:true,score:80,reason:"test".into()}]});
        core.store.put_project(&project).unwrap();(dir,core,project)
    }
    #[tokio::test]async fn files_download_multiple_assets_and_never_install_even_after_mode_switch(){
        let (_dir,core,mut project)=paused_queue();
        project.management_mode="files".into();
        let mut release=project.release.take().unwrap();
        release.assets[0].name="textures.zip".into();
        let mut other=release.assets[0].clone();other.id=8;other.name="model.safetensors".into();other.url="https://github.com/owner/app/releases/download/v1.0.0/model.safetensors".into();
        release.assets.push(other);
        for a in &mut release.assets {super::super::selection::classify_for_mode(a,&project);}
        project.release=Some(release);core.store.put_project(&project).unwrap();
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"selectedAssetId":7})).unwrap()).unwrap();
        assert_eq!(core.store.project(&project.id).unwrap().selected_asset_id,Some(7));
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"assetPattern":"*.safetensors"})).unwrap()).unwrap();
        assert_eq!(core.store.project(&project.id).unwrap().selected_asset_id,Some(8));
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"assetPattern":null})).unwrap()).unwrap();
        assert_eq!(core.store.project(&project.id).unwrap().selected_asset_id,None);
        let first=core.queue_download(&project.id,7).unwrap();let second=core.queue_download(&project.id,8).unwrap();
        assert!(!first.installable);assert!(!second.installable);assert_eq!(core.store.downloads().unwrap().len(),2);
        assert_eq!(core.handle("snapshot",serde_json::json!({})).await.unwrap()["projects"][0]["fileStatus"],"notDownloaded");
        let folder=directory(&core.data_dir,&first.id).unwrap();let file=folder.join(&first.file_name);std::fs::write(&file,b"test file").unwrap();
        core.store.mutate("downloads",&first.id,|d:&mut DownloadRecord|{d.status="completed".into();d.path=Some(file.to_string_lossy().into_owned());Ok(())}).unwrap();
        core.store.mutate("downloads",&second.id,|d:&mut DownloadRecord|{d.status="completed".into();d.path=None;Ok(())}).unwrap();
        assert_eq!(core.handle("snapshot",serde_json::json!({})).await.unwrap()["projects"][0]["fileStatus"],"downloaded");
        assert!(core.retry_download(&first.id).is_err());
        for (action,payload) in [("install_download",serde_json::json!({"id":first.id,"confirmed":true})),
            ("mark_installed",serde_json::json!({"projectId":project.id,"version":"v1"})),
            ("activate_managed_version",serde_json::json!({"projectId":project.id,"downloadId":first.id,"confirmed":true}))] {
            assert!(core.handle(action,payload).await.is_err(),"{action}");
        }
        assert!(core.store.project(&project.id).unwrap().installed_version.is_none());
        assert_eq!(std::fs::read_dir(core.data_dir.join("apps")).unwrap().count(),0);
        core.store.mutate("projects",&project.id,|p:&mut Project|{p.release.as_mut().unwrap().tag="v2".into();Ok(())}).unwrap();
        assert_eq!(core.handle("snapshot",serde_json::json!({})).await.unwrap()["projects"][0]["fileStatus"],"updateAvailable");
        core.store.mutate("projects",&project.id,|p:&mut Project|{p.release.as_mut().unwrap().tag="v1.0.0".into();Ok(())}).unwrap();
        assert!(file.starts_with(&core.data_dir));std::fs::remove_file(&file).unwrap();
        assert_eq!(core.handle("snapshot",serde_json::json!({})).await.unwrap()["projects"][0]["fileStatus"],"missing");
        assert_eq!(core.retry_download(&first.id).unwrap().status,"queued");
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"managementMode":"software"})).unwrap()).unwrap();
        core.store.mutate("downloads",&first.id,|d:&mut DownloadRecord|{d.status="completed".into();Ok(())}).unwrap();
        assert!(core.install_download(&first.id).await.unwrap_err().contains("仅供下载"));
        let mut old=core.store.download(&second.id).unwrap();old.installable=true;core.store.put_download(&old).unwrap();
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"managementMode":"files"})).unwrap()).unwrap();
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"managementMode":"software"})).unwrap()).unwrap();
        assert!(!core.store.download(&second.id).unwrap().installable);
        core.installing.lock().unwrap().insert(project.id.clone());
        assert!(core.update_project(&project.id,serde_json::from_value(serde_json::json!({"managementMode":"files"})).unwrap()).is_err());
        core.installing.lock().unwrap().remove(&project.id);
        core.update_project(&project.id,serde_json::from_value(serde_json::json!({"managementMode":"files"})).unwrap()).unwrap();
        assert!(core.update_project(&project.id,serde_json::from_value(serde_json::json!({"installedVersion":"v3"})).unwrap()).is_err());
        assert!(core.handle("add_project",serde_json::json!({"input":{"repository":"owner/no-network","channel":"stable","preference":"auto","managementMode":"files","installedVersion":"v1"}})).await.is_err());
        assert_eq!(core.store.downloads().unwrap().len(),2);
    }
    #[test]fn external_download_prevents_duplicate_internal_download(){
        let (_dir,core,project)=paused_queue();let queued=core.queue_download(&project.id,7).unwrap();
        core.store.mutate("downloads",&queued.id,|d:&mut DownloadRecord|{d.status="external".into();Ok(())}).unwrap();
        assert_eq!(core.queue_download(&project.id,7).unwrap().id,queued.id);
        assert_eq!(core.store.downloads().unwrap().len(),1);
    }
    #[test]fn queue_cancel_and_retry_preserve_identity_and_publisher_digest(){
        let (_dir,core,project)=paused_queue();let queued=core.queue_download(&project.id,7).unwrap();
        assert_eq!(queued.status,"queued");assert_eq!(core.queue_download(&project.id,7).unwrap().id,queued.id);
        assert_eq!(core.store.downloads().unwrap().len(),1);
        let flag=Arc::new(AtomicBool::new(false));core.active_downloads.lock().unwrap().insert(queued.id.clone(),flag.clone());
        core.store.mutate("downloads",&queued.id,|d:&mut DownloadRecord|{d.status="downloading".into();d.received_bytes=4;Ok(())}).unwrap();
        core.cancel_download(&queued.id).unwrap();assert!(flag.load(Ordering::Acquire));
        assert_eq!(core.store.download(&queued.id).unwrap().status,"cancelled");assert!(core.retry_download(&queued.id).is_err());
        core.active_downloads.lock().unwrap().remove(&queued.id);
        let retried=core.retry_download(&queued.id).unwrap();assert_eq!(retried.id,queued.id);assert_eq!(retried.status,"queued");
        assert_eq!(retried.received_bytes,0);assert!(retried.path.is_none());assert!(retried.sha256.is_none());assert!(!retried.verified);assert!(retried.error.is_none());
        assert_eq!(core.store.digest(&queued.id).unwrap(),Some("a".repeat(64)));assert!(core.retry_download(&queued.id).is_err());
        core.store.mutate("downloads",&queued.id,|d:&mut DownloadRecord|{d.status="failed".into();d.error=Some("interrupted".into());Ok(())}).unwrap();
        assert_eq!(core.retry_download(&queued.id).unwrap().status,"queued");
    }
    #[tokio::test]async fn cancelled_worker_never_requests_or_publishes_a_file(){
        let (_dir,core,project)=paused_queue();let queued=core.queue_download(&project.id,7).unwrap();
        let result=core.download_worker(&queued.id,Arc::new(AtomicBool::new(true))).await;
        assert_eq!(result.unwrap_err(),"下载已取消");
        let folder=directory(&core.data_dir,&queued.id).unwrap();
        assert!(!folder.join(&queued.file_name).exists());assert!(!folder.join(format!("{}.part",queued.file_name)).exists());
        core.cancel_download(&queued.id).unwrap();assert_eq!(core.store.download(&queued.id).unwrap().status,"cancelled");
    }
}
