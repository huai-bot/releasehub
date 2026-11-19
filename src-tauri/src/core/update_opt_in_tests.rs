// Included in update_tests so the established temporary software/package fixtures are shared.
#[tokio::test]
async fn disabling_backups_preserves_existing_restore_point_and_creates_no_snapshot() {
    let (_dir,core,p,target)=fixture();
    let first=package(&core,&p,"v2",&[("app.exe",b"v2")]);
    core.install_confirmed(input(&first,&target,".",Some("v1"))).await.unwrap();
    let backup=core.get_backup(&p.id).unwrap().unwrap();
    let project=core.update_project(&p.id,serde_json::from_value(json!({"backupEnabled":false})).unwrap()).unwrap();
    assert!(!project.backup_enabled);
    let next=package(&core,&project,"v3",&[("app.exe",b"v3")]);
    let preview=core.preview_install(&next.id,None).unwrap();
    assert!(!preview.backup_enabled);assert_eq!(preview.backup_bytes,0);assert!(preview.backup_path.is_none());
    let result=core.install_confirmed(input(&next,&target,".",Some("v2"))).await.unwrap();
    assert!(!result["message"].as_str().unwrap().contains("已备份"));
    assert_eq!(core.get_backup(&p.id).unwrap().unwrap().path,backup.path);
    assert_eq!(fs::read_dir(core.backup_folder(&p.id).unwrap()).unwrap().count(),1);
    assert_eq!(fs::read_dir(core.data_dir.join("temp")).unwrap().count(),0);
    core.restore_backup(&p.id).unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"old fixture not executable");
    assert!(!core.store.project(&p.id).unwrap().backup_enabled);
}

#[tokio::test]
async fn no_backup_partial_failure_keeps_files_reports_unknown_version_and_allows_retry() {
    let (_dir,core,p,target)=fixture();
    core.update_project(&p.id,serde_json::from_value(json!({"backupEnabled":false})).unwrap()).unwrap();
    let broken=package(&core,&p,"v2",&[("aaa-added.txt",b"partial"),("app.exe",b"v2"),("config",b"collides with directory")]);
    let error=core.install_confirmed(input(&broken,&target,".",Some("v1"))).await.unwrap_err();
    assert!(error.contains("未开启备份"));assert!(error.contains("部分更新"));assert!(!error.contains("已恢复"));
    assert_eq!(fs::read(target.join("aaa-added.txt")).unwrap(),b"partial");
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"v2");
    assert_eq!(fs::read(target.join("config/settings.json")).unwrap(),b"user configuration");
    let project=core.store.project(&p.id).unwrap();
    assert!(project.installed_version.is_none());assert_eq!(project.status,"error");assert!(project.error.is_some());
    let record=core.store.download(&broken.id).unwrap();assert_eq!(record.status,"completed");assert!(record.error.is_some());
    assert!(core.store.journals().unwrap().is_empty());assert!(core.get_backup(&p.id).unwrap().is_none());
    assert!(fs::read_dir(core.data_dir.join("backups")).unwrap().next().is_none());
    core.preview_install(&broken.id,None).unwrap();
    let good=package(&core,&project,"v3",&[("app.exe",b"v3")]);
    core.install_confirmed(input(&good,&target,".",None)).await.unwrap();
    assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(),Some("v3"));
}

#[test]
fn no_backup_restart_preserves_target_and_cleans_only_staging_after_data_move() {
    let (dir,core,mut p,target)=fixture();p.backup_enabled=false;core.store.put_project(&p).unwrap();
    let mut d=package(&core,&p,"v2",&[("app.exe",b"v2")]);
    let stage=core.data_dir.join("temp").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&stage).unwrap();fs::write(stage.join("staged.txt"),b"package").unwrap();
    let j=InstallJournal{backup:BackupRecord{project_id:p.id.clone(),download_id:d.id.clone(),version:p.installed_version.clone(),installed_source:p.installed_source.clone(),target_path:target.to_string_lossy().into_owned(),path:String::new(),created_at:now(),bytes:0,kind:"portable".into(),phase:"installing".into(),error:None},original_project:p.clone(),previous_download:d.clone(),target_empty:false,stage_path:stage.to_string_lossy().into_owned(),snapshot_enabled:false,snapshot_files:vec![],original_files:vec![]};
    // Even a direct attempt to restore this metadata must never clear a directory.
    assert!(core.restore_journal(&mut j.clone()).is_err());
    core.store.put_journal(&j).unwrap();d.status="installing".into();core.store.put_download(&d).unwrap();
    fs::write(target.join("app.exe"),b"partial new bytes").unwrap();fs::write(target.join("partial.txt"),b"preserve").unwrap();
    let data=core.data_dir.clone();let moved=dir.path().join("moved-data");drop(core);
    let temporary_root=fs::canonicalize(dir.path()).unwrap();assert!(data.starts_with(&temporary_root));assert_eq!(fs::canonicalize(moved.parent().unwrap()).unwrap(),temporary_root);
    fs::rename(&data,&moved).unwrap();
    let reopened=Core::new(moved,true,Arc::new(|_,_|{})).unwrap();
    assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"partial new bytes");assert!(target.join("partial.txt").exists());
    assert!(reopened.store.project(&p.id).unwrap().installed_version.is_none());
    assert!(reopened.store.project(&p.id).unwrap().error.unwrap().contains("安装被中断"));
    assert_eq!(reopened.store.download(&d.id).unwrap().status,"completed");
    assert!(reopened.store.journals().unwrap().is_empty());assert!(reopened.get_backup(&p.id).unwrap().is_none());
    assert!(fs::read_dir(reopened.data_dir.join("temp")).unwrap().next().is_none());
    reopened.preview_install(&d.id,None).unwrap();
}

#[test]
fn legacy_snapshot_journal_without_new_field_still_recovers() {
    let (_dir,core,p,target)=fixture();let d=package(&core,&p,"v2",&[("app.exe",b"v2")]);
    core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();
    let backup=core.get_backup(&p.id).unwrap().unwrap();let j=core.read_manifest(&backup).unwrap();
    let mut old=serde_json::to_value(j).unwrap();old.as_object_mut().unwrap().remove("snapshotEnabled");old["backup"]["phase"]=json!("installing");
    let mut legacy:InstallJournal=serde_json::from_value(old).unwrap();assert!(legacy.snapshot_enabled);
    core.restore_journal(&mut legacy).unwrap();assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"old fixture not executable");
}

#[test]
fn no_backup_managed_migration_preserves_configuration_and_rebases_executable() {
    let (dir,core,mut p,_target)=fixture();
    let old=core.data_dir.join("apps").join(&p.id).join("v1-old");fs::create_dir_all(&old).unwrap();
    fs::write(old.join("app.exe"),b"old app").unwrap();fs::write(old.join("settings.json"),b"old config").unwrap();
    p.backup_enabled=false;p.installed_source=Some("managed".into());p.installed_path=Some(old.to_string_lossy().into_owned());p.executable_path=Some(old.join("app.exe").to_string_lossy().into_owned());core.store.put_project(&p).unwrap();
    let target=dir.path().join("migrated");fs::create_dir(&target).unwrap();
    let d=package(&core,&p,"v2",&[("app.exe",b"new app")]);core.perform_install(&input(&d,&target,".",Some("v1"))).unwrap();
    assert_eq!(fs::read(target.join("settings.json")).unwrap(),b"old config");assert_eq!(fs::read(target.join("app.exe")).unwrap(),b"new app");
    assert_eq!(fs::read(old.join("app.exe")).unwrap(),b"old app");
    assert_eq!(core.project_executable(&p.id).unwrap(),fs::canonicalize(target.join("app.exe")).unwrap());
    assert!(core.get_backup(&p.id).unwrap().is_none());
}

#[test]
fn backup_choice_roundtrips_and_legacy_import_preserves_existing_choice() {
    let (dir,core,p,_target)=fixture();
    let exported=core.export_library().unwrap();let mut json:Value=serde_json::from_str(&exported).unwrap();
    assert_eq!(json["projects"][0]["backupEnabled"],true);
    let target=Core::new(dir.path().join("imported"),true,Arc::new(|_,_|{})).unwrap();target.import_library(&exported).unwrap();
    let imported=target.store.projects().unwrap().remove(0);assert!(imported.backup_enabled);
    json["projects"][0].as_object_mut().unwrap().remove("backupEnabled");
    target.import_library(&json.to_string()).unwrap();assert!(target.store.project(&imported.id).unwrap().backup_enabled);
    let legacy=Core::new(dir.path().join("legacy"),true,Arc::new(|_,_|{})).unwrap();legacy.import_library(&json.to_string()).unwrap();assert!(!legacy.store.projects().unwrap()[0].backup_enabled);
    let mut old=serde_json::to_value(p).unwrap();old.as_object_mut().unwrap().remove("backupEnabled");assert!(!serde_json::from_value::<Project>(old).unwrap().backup_enabled);
    let project=target.update_project(&imported.id,serde_json::from_value(json!({"managementMode":"files","backupEnabled":true})).unwrap()).unwrap();assert!(!project.backup_enabled);
}

#[test]
fn changing_backup_choice_preserves_selected_release_asset() {
    let (_dir,core,mut p,_target)=fixture();
    let assets=vec![987654321u64,987654322].into_iter().map(|id| json!({"id":id,"name":format!("app-windows-x64-{id}.zip"),"size":100,"url":format!("https://github.com/fixture/app/releases/download/v2/{id}.zip"),"contentType":"application/zip","digest":null,"kind":"portable","compatible":true,"score":80,"reason":"fixture","downloadable":true,"installable":true})).collect::<Vec<_>>();
    p.release=Some(serde_json::from_value(json!({"id":2,"tag":"v2","name":"v2","body":"fixture","publishedAt":"2026-11-19T00:00:00Z","prerelease":false,"url":"https://github.com/fixture/app/releases/tag/v2","assets":assets})).unwrap());
    p.selected_asset_id=Some(987654321);core.store.put_project(&p).unwrap();
    let updated=core.update_project(&p.id,serde_json::from_value(json!({"backupEnabled":false})).unwrap()).unwrap();
    assert!(!updated.backup_enabled);assert_eq!(updated.selected_asset_id,p.selected_asset_id);assert!(updated.release.is_some());
}
