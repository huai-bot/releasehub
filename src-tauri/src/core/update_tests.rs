use super::*;
include!("update_opt_in_tests.rs");
use sha2::{Digest,Sha256};
fn fixture()->(tempfile::TempDir,Arc<Core>,Project,PathBuf) {
    let dir=tempfile::tempdir().unwrap();let core=Core::new(dir.path().join("data"),true,Arc::new(|_,_|{})).unwrap();
    let target=dir.path().join("software");fs::create_dir(&target).unwrap();
    fs::create_dir(target.join("config")).unwrap();fs::write(target.join("config/settings.json"),b"user configuration").unwrap();
    fs::write(target.join("app.exe"),b"old fixture not executable").unwrap();
    let mut p=test_project("fixture","app");p.installed_version=Some("v1".into());p.installed_source=Some("manual".into());p.installed_path=Some(target.to_string_lossy().into_owned());
    p.backup_enabled = true; // 显式开启备份以满足测试断言
    core.store.put_project(&p).unwrap();(dir,core,p,target)
}
fn package(core:&Core,p:&Project,tag:&str,files:&[(&str,&[u8])])->DownloadRecord {
    let id=uuid::Uuid::new_v4().to_string();let folder=super::super::downloads::directory(&core.data_dir,&id).unwrap();let source=folder.join("app.zip");
    let mut writer=zip::ZipWriter::new(fs::File::create(&source).unwrap());
    for (name,bytes) in files {writer.start_file(*name,zip::write::SimpleFileOptions::default()).unwrap();writer.write_all(bytes).unwrap();}
    writer.finish().unwrap();let bytes=fs::read(&source).unwrap();let time=now();
    let d=DownloadRecord{id,project_id:p.id.clone(),project_name:p.name.clone(),asset_id:1,file_name:"app.zip".into(),tag:tag.into(),
        url:format!("https://github.com/fixture/app/releases/download/{tag}/app.zip"),kind:"portable".into(),installable:true,total_bytes:bytes.len()as u64,received_bytes:bytes.len()as u64,
        status:"completed".into(),path:Some(source.to_string_lossy().into_owned()),sha256:Some(format!("{:x}",Sha256::digest(&bytes))),verified:true,error:None,created_at:time.clone(),updated_at:time};
    core.store.put_download(&d).unwrap();d
}
fn input(d:&DownloadRecord,target:&Path,root:&str,version:Option<&str>)->InstallInput {
    InstallInput{id:d.id.clone(),confirmed:true,target_path:target.to_string_lossy().into_owned(),archive_root:Some(root.into()),previous_version:Some(version.map(str::to_owned))}
}
#[tokio::test]async fn confirmed_overlay_full_latest_backup_restore_and_download_deletion(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"new fixture"),("new.dll",b"added")]);
    let preview=core.preview_install(&d.id,None).unwrap();assert_eq!(preview.previous_version.as_deref(),Some("v1"));assert_eq!(preview.suggested_root.as_deref(),Some("."));assert!(preview.backup_bytes>0);
    assert!(core.handle("install_download",json!({"id":d.id,"targetPath":target,"archiveRoot":".","confirmed":false})).await.is_err());
    core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"new fixture");assert_eq!(fs::read(target.join("config/settings.json")).unwrap(),b"user configuration");
    let backup=core.get_backup(&p.id).unwrap().unwrap();assert_eq!(backup.phase,"ready");assert_eq!(fs::read(Path::new(&backup.path).join("target/app.exe")).unwrap(),b"old fixture not executable");
    assert!(core.handle("delete_download",json!({"id":d.id})).await.is_err());
    assert_eq!(core.handle("delete_download",json!({"id":d.id,"confirmed":true})).await.unwrap(),Value::Null);
    assert!(core.store.optional_download(&d.id).unwrap().is_none());assert!(!Path::new(d.path.as_deref().unwrap()).exists());assert!(target.join("new.dll").exists());assert!(Path::new(&backup.path).exists());
    assert!(core.handle("restore_backup",json!({"projectId":p.id})).await.is_err());
    core.handle("restore_backup",json!({"projectId":p.id,"confirmed":true})).await.unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"old fixture not executable");assert!(!target.join("new.dll").exists());assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v1"));
    assert!(core.store.optional_download(&d.id).unwrap().is_none());
    assert!(core.handle("delete_backup",json!({"projectId":p.id})).await.is_err());core.handle("delete_backup",json!({"projectId":p.id,"confirmed":true})).await.unwrap();
    assert!(!Path::new(&backup.path).exists());assert!(core.get_backup(&p.id).unwrap().is_none());assert!(target.join("app.exe").exists());
}
#[tokio::test]async fn latest_backup_replaces_previous_only_after_success(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    let first=core.get_backup(&p.id).unwrap().unwrap();let d=package(&core,&p,"v3",&[("app.exe",b"v3")]);core.install_confirmed(input(&d,&target,".",Some("v2"))).await.unwrap();
    let second=core.get_backup(&p.id).unwrap().unwrap();assert_ne!(first.path,second.path);assert!(!Path::new(&first.path).exists());assert_eq!(second.version.as_deref(),Some("v2"));
    assert_eq!(fs::read(Path::new(&second.path).join("target/app.exe")).unwrap(),b"v2");core.restore_backup(&p.id).unwrap();assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");
}
#[tokio::test]async fn package_conflict_rolls_back_added_files_and_keeps_previous_backup(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    let before=core.get_backup(&p.id).unwrap().unwrap();let d=package(&core,&p,"v3",&[("aaa-added.txt",b"added"),("config",b"file collides directory")]);
    let error=core.install_confirmed(input(&d,&target,".",Some("v2"))).await.unwrap_err();assert!(error.contains("已恢复原目录"),"{error}");
    assert!(!target.join("aaa-added.txt").exists());assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");assert_eq!(core.get_backup(&p.id).unwrap().unwrap().path,before.path);assert!(core.store.journals().unwrap().is_empty());
}
#[tokio::test]async fn restart_recovers_durable_installing_phase_and_version(){
    let (dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2"),("new.dll",b"added")]);core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    let backup=core.get_backup(&p.id).unwrap().unwrap();let mut journal=core.read_manifest(&backup).unwrap();journal.backup.phase="installing".into();core.store.put_journal(&journal).unwrap();
    fs::write(target.join("partial.txt"),b"interrupted install").unwrap();let data=core.data_dir.clone();drop(core);
    let reopened=Core::new(data,true,Arc::new(|_,_|{})).unwrap();assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"old fixture not executable");assert!(!target.join("partial.txt").exists());assert!(!target.join("new.dll").exists());
    assert_eq!(reopened.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v1"));assert!(reopened.store.journals().unwrap().is_empty());assert!(dir.path().exists());
}
#[tokio::test]async fn damaged_backup_is_not_restored_and_recovery_blocks_destructive_commands(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    let backup=core.get_backup(&p.id).unwrap().unwrap();fs::write(Path::new(&backup.path).join("target/app.exe"),b"tampered").unwrap();
    assert!(core.restore_backup(&p.id).unwrap_err().contains("备份缺失或内容已改变"));assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");
    assert_eq!(core.get_backup(&p.id).unwrap().unwrap().phase,"recoveryRequired");assert!(core.delete_backup(&p.id).is_err());assert!(core.delete_download(&d.id).is_err());
    assert!(core.preview_install(&d.id,None).is_err());assert!(core.project_executable(&p.id).is_err());assert!(core.handle("remove_project",json!({"id":p.id})).await.is_err());
}
#[tokio::test]async fn preparation_restart_never_restores_incomplete_snapshot(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    let old=core.get_backup(&p.id).unwrap().unwrap();let mut journal=core.read_manifest(&old).unwrap();
    let candidate=core.backup_folder(&p.id).unwrap().join(uuid::Uuid::new_v4().to_string());fs::create_dir(&candidate).unwrap();fs::create_dir(candidate.join("target")).unwrap();
    journal.backup.path=candidate.to_string_lossy().into_owned();journal.backup.phase="preparing".into();core.store.put_journal(&journal).unwrap();let data=core.data_dir.clone();drop(core);
    let reopened=Core::new(data,true,Arc::new(|_,_|{})).unwrap();assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");assert_eq!(reopened.get_backup(&p.id).unwrap().unwrap().path,old.path);assert!(!candidate.exists());
}
#[tokio::test]async fn archive_root_is_explicit_and_wrapper_is_never_guessed(){
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("wrapper/app.exe",b"v2"),("wrapper/readme.txt",b"data")]);let preview=core.preview_install(&d.id,None).unwrap();
    assert_eq!(preview.archive_roots,vec![".","wrapper"]);assert!(preview.requires_root_selection);assert!(preview.suggested_root.is_none());
    let mut request=input(&d,&target,".",Some("v1"));request.archive_root=None;assert!(core.install_confirmed(request).await.is_err());
    assert!(core.install_confirmed(input(&d,&target,"../outside",Some("v1"))).await.is_err());assert!(core.install_confirmed(input(&d,&target,"wrapper",Some("outdated"))).await.is_err());
    core.install_confirmed(input(&d,&target,"wrapper",Some("v1"))).await.unwrap();assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");assert!(!target.join("wrapper").exists());
}
#[tokio::test]async fn legacy_managed_requires_explicit_empty_migration_and_restores_original(){
    let (dir,core,mut p,target)=fixture();let managed=core.data_dir.join("apps").join(&p.id);fs::create_dir(&managed).unwrap();let old=managed.join("v1-old");fs::create_dir(&old).unwrap();
    fs::write(old.join("settings.json"),b"legacy settings").unwrap();fs::write(old.join("app.exe"),b"legacy v1").unwrap();p.installed_path=Some(old.to_string_lossy().into_owned());p.installed_source=Some("managed".into());core.store.put_project(&p).unwrap();
    let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);assert!(core.preview_install(&d.id,None).is_err());assert!(core.preview_install(&d.id,Some(target.to_str().unwrap())).is_err());
    let migrated=dir.path().join("migration");fs::create_dir(&migrated).unwrap();let preview=core.preview_install(&d.id,Some(migrated.to_str().unwrap())).unwrap();assert!(preview.legacy_managed);
    core.install_confirmed(input(&d,&migrated,".",Some("v1"))).await.unwrap();assert_eq!(fs::read(migrated.join("settings.json")).unwrap(),b"legacy settings");assert_eq!(fs::read(old.join("app.exe")).unwrap(),b"legacy v1");
    let backup=core.get_backup(&p.id).unwrap().unwrap();assert!(Path::new(&backup.path).join("original/settings.json").exists());fs::write(old.join("settings.json"),b"changed old").unwrap();
    core.restore_backup(&p.id).unwrap();assert!(fs::read_dir(&migrated).unwrap().next().is_none());assert_eq!(fs::read(old.join("settings.json")).unwrap(),b"legacy settings");assert_eq!(core.store.project(&p.id).unwrap().installed_source.as_deref(),Some("managed"));
}
#[test]fn portable_backup_rebases_only_current_data_and_preserves_external_target(){
    let (dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();
    let old_backup=core.get_backup(&p.id).unwrap().unwrap();let old_data=core.data_dir.clone();let moved=dir.path().join("moved-data");drop(core);
    let temporary_root=fs::canonicalize(dir.path()).unwrap();assert!(old_data.starts_with(&temporary_root)&&fs::canonicalize(moved.parent().unwrap()).unwrap()==temporary_root);fs::rename(&old_data,&moved).unwrap();
    fs::create_dir_all(&old_backup.path).unwrap();fs::write(Path::new(&old_backup.path).join("old-location-marker"),b"never touch old location").unwrap();
    let reopened=Core::new(moved,true,Arc::new(|_,_|{})).unwrap();let backup=reopened.get_backup(&p.id).unwrap().unwrap();
    assert!(Path::new(&backup.path).starts_with(&reopened.data_dir));assert_eq!(backup.target_path,old_backup.target_path);reopened.restore_backup(&p.id).unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"old fixture not executable");reopened.delete_backup(&p.id).unwrap();assert!(Path::new(&old_backup.path).join("old-location-marker").exists());
}
#[test]fn portable_legacy_journal_recovers_current_managed_tree_after_data_move(){
    let (dir,core,mut p,_target)=fixture();let old=core.data_dir.join("apps").join(&p.id).join("v1-old");fs::create_dir_all(&old).unwrap();
    fs::write(old.join("app.exe"),b"legacy v1").unwrap();p.installed_source=Some("managed".into());p.installed_path=Some(old.to_string_lossy().into_owned());p.executable_path=Some(old.join("app.exe").to_string_lossy().into_owned());core.store.put_project(&p).unwrap();
    let target=dir.path().join("migration");fs::create_dir(&target).unwrap();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();
    let b=core.get_backup(&p.id).unwrap().unwrap();let mut journal=core.read_manifest(&b).unwrap();journal.backup.phase="installing".into();core.store.put_journal(&journal).unwrap();
    fs::write(old.join("app.exe"),b"changed managed").unwrap();let data=core.data_dir.clone();let moved=dir.path().join("moved-data");drop(core);let temporary_root=fs::canonicalize(dir.path()).unwrap();assert!(data.starts_with(&temporary_root)&&fs::canonicalize(moved.parent().unwrap()).unwrap()==temporary_root);fs::rename(&data,&moved).unwrap();
    let reopened=Core::new(moved,true,Arc::new(|_,_|{})).unwrap();let project=reopened.store.project(&p.id).unwrap();
    assert_eq!(project.installed_source.as_deref(),Some("managed"));let recovered=Path::new(project.installed_path.as_deref().unwrap());assert!(recovered.starts_with(&reopened.data_dir));assert_eq!(fs::read(recovered.join("app.exe")).unwrap(),b"legacy v1");
    assert_eq!(reopened.project_executable(&p.id).unwrap(),fs::canonicalize(recovered.join("app.exe")).unwrap());assert!(fs::read_dir(target).unwrap().next().is_none());assert!(reopened.store.journals().unwrap().is_empty());
}
#[test]fn portable_recovery_never_guesses_moved_external_software_path(){
    let (dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();
    let b=core.get_backup(&p.id).unwrap().unwrap();let mut journal=core.read_manifest(&b).unwrap();journal.backup.phase="installing".into();core.store.put_journal(&journal).unwrap();
    let data=core.data_dir.clone();let moved=dir.path().join("moved-data");let software_moved=dir.path().join("software-moved");drop(core);
    let temporary_root=fs::canonicalize(dir.path()).unwrap();assert!(data.starts_with(&temporary_root)&&fs::canonicalize(&target).unwrap().starts_with(&temporary_root)&&fs::canonicalize(moved.parent().unwrap()).unwrap()==temporary_root&&fs::canonicalize(software_moved.parent().unwrap()).unwrap()==temporary_root);fs::rename(&data,&moved).unwrap();fs::rename(&target,&software_moved).unwrap();
    let reopened=Core::new(moved,true,Arc::new(|_,_|{})).unwrap();let backup=reopened.get_backup(&p.id).unwrap().unwrap();assert_eq!(backup.target_path,b.target_path);assert_eq!(backup.phase,"recoveryRequired");
    assert_eq!(fs::read(software_moved.join("app.exe")).unwrap(),b"v2");assert!(!target.exists());assert!(reopened.delete_backup(&p.id).is_err());
}
#[test]fn target_rules_require_explicit_path_and_reject_unsafe_overlap(){
    let (dir,core,mut p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);assert!(core.preview_install(&d.id,Some(core.data_dir.to_str().unwrap())).is_err());assert!(core.preview_install(&d.id,Some(dir.path().to_str().unwrap())).is_err());
    assert!(core.preview_install(&d.id,Some("relative-path")).is_err());assert!(core.preview_install(&d.id,Some(dir.path().join("missing").to_str().unwrap())).is_err());
    p.installed_path=None;core.store.put_project(&p).unwrap();assert!(core.preview_install(&d.id,None).unwrap_err().contains("明确选择"));assert!(core.preview_install(&d.id,Some(target.to_str().unwrap())).unwrap_err().contains("先明确记录原软件路径"));
    let other=target.join("other");fs::create_dir(&other).unwrap();let mut q=test_project("fixture","other");q.installed_path=Some(other.to_string_lossy().into_owned());core.store.put_project(&q).unwrap();assert!(core.preview_install(&d.id,Some(target.to_str().unwrap())).is_err());
}
#[test]fn executable_selection_records_parent_and_preserves_encompassing_root(){
    let (dir,core,mut p,target)=fixture();p.installed_path=None;core.store.put_project(&p).unwrap();
    let exe=target.join("app.exe");let preview=core.preview_project_executable(&p.id,exe.to_str().unwrap()).unwrap();
    assert_eq!(Path::new(preview["installedPath"].as_str().unwrap()),fs::canonicalize(&target).unwrap());
    assert!(core.store.project(&p.id).unwrap().installed_path.is_none());
    core.save_project_executable(&p.id,exe.to_str().unwrap()).unwrap();
    assert_eq!(core.project_executable(&p.id).unwrap(),fs::canonicalize(&exe).unwrap());
    let saved=core.store.project(&p.id).unwrap();assert_eq!(Path::new(saved.installed_path.as_deref().unwrap()),fs::canonicalize(&target).unwrap());
    assert!(core.save_project_executable(&p.id,target.join("config/settings.json").to_str().unwrap()).is_err());
    let bin=target.join("bin");fs::create_dir(&bin).unwrap();let nested=bin.join("APP.EXE");fs::write(&nested,b"fixture not executable").unwrap();
    core.save_project_executable(&p.id,nested.to_str().unwrap()).unwrap();
    let same=core.update_project(&p.id,serde_json::from_value(json!({"installedPath":core.store.project(&p.id).unwrap().installed_path})).unwrap()).unwrap();
    assert_eq!(Path::new(same.installed_path.as_deref().unwrap()),fs::canonicalize(&target).unwrap());assert!(same.executable_path.is_some());
    let other=dir.path().join("other");fs::create_dir(&other).unwrap();let other_exe=other.join("app.exe");fs::write(&other_exe,b"fixture not executable").unwrap();
    core.save_project_executable(&p.id,other_exe.to_str().unwrap()).unwrap();
    assert_eq!(core.project_executable(&p.id).unwrap(),fs::canonicalize(&other_exe).unwrap());
    let changed=core.update_project(&p.id,serde_json::from_value(json!({"installedPath":target})).unwrap()).unwrap();assert!(changed.executable_path.is_none());
    let unsafe_exe=core.data_dir.join("app.exe");fs::write(&unsafe_exe,b"fixture").unwrap();assert!(core.save_project_executable(&p.id,unsafe_exe.to_str().unwrap()).is_err());
    assert!(core.save_project_executable(&p.id,"relative.exe").is_err());
}
#[tokio::test]async fn mark_installed_preserves_omitted_paths_and_commits_executable_atomically(){
    let (dir,core,p,target)=fixture();core.save_project_executable(&p.id,target.join("app.exe").to_str().unwrap()).unwrap();
    core.handle("mark_installed",json!({"projectId":p.id,"version":"v2"})).await.unwrap();
    let saved=core.store.project(&p.id).unwrap();assert_eq!(saved.installed_version.as_deref(),Some("v2"));assert!(saved.executable_path.is_some());
    let other=dir.path().join("chosen");fs::create_dir(&other).unwrap();let exe=other.join("app.exe");fs::write(&exe,b"fixture not executable").unwrap();
    core.handle("mark_installed",json!({"projectId":p.id,"version":"v3","executablePath":exe})).await.unwrap();
    let saved=core.store.project(&p.id).unwrap();assert_eq!(saved.installed_version.as_deref(),Some("v3"));assert_eq!(core.project_executable(&p.id).unwrap(),fs::canonicalize(&exe).unwrap());
    assert_eq!(Path::new(saved.installed_path.as_deref().unwrap()),fs::canonicalize(&other).unwrap());
    assert!(core.handle("mark_installed",json!({"projectId":p.id,"version":"v4","executablePath":other.join("missing.exe")})).await.is_err());
    assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v3"));
}
#[test]fn installers_backup_files_before_wizard_and_never_claim_registry_rollback(){
    for extension in ["exe","msi"] {
        let (_dir,core,p,target)=fixture();let mut d=package(&core,&p,"v2",&[("unused",b"fixture")]);
        let folder=Path::new(d.path.as_deref().unwrap()).parent().unwrap();let source=folder.join(format!("setup.{extension}"));fs::write(&source,b"not a real installer").unwrap();
        d.kind="installer".into();d.file_name=format!("setup.{extension}");d.path=Some(source.to_string_lossy().into_owned());d.total_bytes=20;d.received_bytes=20;d.sha256=Some(format!("{:x}",Sha256::digest(b"not a real installer")));core.store.put_download(&d).unwrap();
        let result=core.perform_install_with(&input(&d,&target,".",Some("v1")),|path|{
            assert_eq!(path,source);let journals=core.store.journals().unwrap();assert_eq!(journals[0].backup.phase,"installing");assert!(Path::new(&journals[0].backup.path).join("target/config/settings.json").exists());Ok(())
        }).unwrap();
        assert_eq!(result["installed"],false);assert_eq!(result["kind"],"installer");assert!(result["message"].as_str().unwrap().contains("注册表"));
        assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v1"));assert_eq!(core.store.download(&d.id).unwrap().status,"completed");assert_eq!(core.get_backup(&p.id).unwrap().unwrap().phase,"ready");
    }
}
#[test]fn failed_wizard_launch_restores_files_and_retains_previous_snapshot(){
    let (_dir,core,p,target)=fixture();let mut d=package(&core,&p,"v2",&[("app.exe",b"v2")]);core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();let old=core.get_backup(&p.id).unwrap().unwrap();
    let folder=Path::new(d.path.as_deref().unwrap()).parent().unwrap();let source=folder.join("setup.exe");fs::write(&source,b"fixture").unwrap();d.kind="installer".into();d.file_name="setup.exe".into();d.path=Some(source.to_string_lossy().into_owned());d.total_bytes=7;d.sha256=Some(format!("{:x}",Sha256::digest(b"fixture")));d.status="completed".into();core.store.put_download(&d).unwrap();
    let error=core.perform_install_with(&input(&d,&target,".",Some("v2")),|_|{fs::write(target.join("partial-wizard-file"),b"partial").unwrap();Err("fixture wizard launch failed".into())}).unwrap_err();
    assert!(error.contains("已恢复原目录"));assert!(!target.join("partial-wizard-file").exists());assert_eq!(core.get_backup(&p.id).unwrap().unwrap().path,old.path);assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v2"));
}
#[cfg(windows)]#[test]fn locked_file_is_rejected_before_any_backup_or_mutation(){
    use std::os::windows::fs::OpenOptionsExt;let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);
    let _lock=fs::OpenOptions::new().read(true).share_mode(0).open(target.join("app.exe")).unwrap();assert!(core.perform_install(&input(&d,&target,".",Some("v1"))).is_err());assert!(core.get_backup(&p.id).unwrap().is_none());assert!(core.store.journals().unwrap().is_empty());
}
#[tokio::test]async fn backup_disabled_avoids_snapshot_and_journal_creation(){
    let (_dir,core,mut p,target)=fixture();
    p.backup_enabled = false;
    core.store.put_project(&p).unwrap();
    let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);
    let preview=core.preview_install(&d.id,None).unwrap();
    assert_eq!(preview.backup_enabled,false);
    assert_eq!(preview.backup_bytes,0);
    core.install_confirmed(input(&d,&target,".",Some("v1"))).await.unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");
    assert!(core.get_backup(&p.id).unwrap().is_none());
    assert!(core.store.journals().unwrap().is_empty());
}
#[cfg(any(windows,unix))]#[test]fn links_in_target_or_download_are_never_followed_or_deleted(){
    let (dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);let external=dir.path().join("outside.txt");fs::write(&external,b"outside untouched").unwrap();let link=target.join("linked.txt");
    #[cfg(windows)]let made=std::os::windows::fs::symlink_file(&external,&link);
    #[cfg(unix)]let made=std::os::unix::fs::symlink(&external,&link);
    if made.is_err(){return;}assert!(core.preview_install(&d.id,None).is_err());assert!(clear_tree(&target).is_err());assert_eq!(fs::read(&external).unwrap(),b"outside untouched");
    fs::remove_file(&link).unwrap();let package=Path::new(d.path.as_deref().unwrap());fs::remove_file(package).unwrap();
    #[cfg(windows)]std::os::windows::fs::symlink_file(&external,package).unwrap();
    #[cfg(unix)]std::os::unix::fs::symlink(&external,package).unwrap();
    assert!(core.delete_download(&d.id).is_err());assert!(core.preview_install(&d.id,None).is_err());assert_eq!(fs::read(&external).unwrap(),b"outside untouched");
}
