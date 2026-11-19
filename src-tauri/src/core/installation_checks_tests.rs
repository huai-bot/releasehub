use super::*;
use chrono::Duration;
use serde_json::json;
use std::path::Path;
use InstallationStatus::*;

fn new_core(path: &Path) -> Arc<Core> { Core::new(path.to_path_buf(), true, Arc::new(|_, _| {})).unwrap() }
fn fixture() -> (tempfile::TempDir, Arc<Core>, Project, DownloadRecord) {
    let dir = tempfile::tempdir().unwrap();
    let core = new_core(dir.path());
    let mut p = test_project("fixture", "install-verify");
    p.installed_version = Some("v1.0".into());
    core.store.put_project(&p).unwrap();
    let d = serde_json::from_value::<DownloadRecord>(json!({
        "id": uuid::Uuid::new_v4().to_string(), "projectId": p.id, "projectName": p.name,
        "assetId": 2, "fileName": "fixture.msi", "tag": "v2.0", "url": "https://github.com/fixture/install-verify/releases/download/v2.0/fixture.msi",
        "kind": "installer", "installable": true, "totalBytes": 10, "receivedBytes": 10,
        "status": "completed", "verified": false, "createdAt": now(), "updatedAt": now()
    })).unwrap();
    core.store.put_download(&d).unwrap();
    (dir, core, p, d)
}
fn identity(key: &str) -> Identity {
    Identity::Registry { hive_key: key.into(), name: "fixture".into(), publisher: "fixture publisher".into(), anchor: RegistryAnchor::Repository }
}
fn observation(observed: &str) -> Probe {
    Probe { registry_complete: true, executable_read: false, observations: vec![Observation { identity: identity("HKCU\\fixture"), version: Some(observed.into()) }] }
}
fn task(core: &Core, p: &Project, d: &DownloadRecord, baseline: Probe, process: bool) -> CheckRecord {
    let t = core.prepare_installation_check(p, d, baseline);
    core.store.begin_installation_check(&t, None).unwrap();
    core.store.finish_installer_launch(&t.check.id, None, process, false).unwrap();
    core.store.installation_record(&t.check.id).unwrap()
}
fn since(t: &CheckRecord, seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&t.check.started_at).unwrap().with_timezone(&Utc) + Duration::seconds(seconds)
}
fn poll(core: &Core, t: &CheckRecord, seconds: i64, force: bool, boot: Option<&str>, observed: &Probe) -> InstallationCheck {
    core.verify_installations_with(Some(&t.check.project_id), force, None, since(t, seconds), boot, |_, _| observed.clone())
        .unwrap().into_iter().find(|c| c.id == t.check.id).unwrap()
}
fn expect_version(core: &Core, p: &Project, version: &str) { assert_eq!(core.store.project(&p.id).unwrap().installed_version.as_deref(), Some(version)); }

#[test]
fn normalized_exact_change_records_target_and_binding() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1.0.0.0"), false);
    let check = poll(&core, &t, 5, false, None, &observation("V2.0.0.0"));
    assert_eq!(check.status, Verified);
    assert_eq!(check.source.as_deref(), Some("registry"));
    assert_eq!(check.detected_version.as_deref(), Some("V2.0.0.0"));
    expect_version(&core, &p, "v2.0");
    assert_eq!(core.store.download(&d.id).unwrap().status, "installed");
    assert_eq!(core.store.install_binding(&p.id).unwrap().unwrap().identities, vec![identity("HKCU\\fixture")]);
    let public = serde_json::to_string(&check).unwrap();
    assert!(!public.contains("HKCU"));
    assert!(!public.contains("fixture publisher"));
}

#[test]
fn zero_exit_from_outer_wizard_needs_a_later_changed_version() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), true);
    assert_eq!(poll(&core, &t, 5, false, None, &observation("2" )).status, Waiting);
    assert!(core.store.record_installer_exit(&t.check.id, ProcessExit::Code(0), None).unwrap());
    assert_eq!(poll(&core, &t, 6, false, None, &observation("1")).status, Detecting);
    assert_eq!(poll(&core, &t, 11, false, None, &observation("3")).status, Detecting);
    expect_version(&core, &p, "v1.0");
    assert_eq!(core.store.download(&d.id).unwrap().status, "completed");
    assert_eq!(poll(&core, &t, 16, false, None, &observation("2")).status, Verified);
}

#[test]
fn long_running_installer_times_out_without_losing_exit_monitoring() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), true);
    assert_eq!(poll(&core, &t, 899, false, None, &observation("2")).status, Waiting);
    assert_eq!(poll(&core, &t, 900, false, None, &observation("2")).status, Unconfirmed);
    assert_eq!(poll(&core, &t, 901, true, None, &observation("2")).status, Unconfirmed);
    expect_version(&core, &p, "v1.0");
    assert!(core.store.record_installer_exit(&t.check.id, ProcessExit::Code(3010), Some("original".into())).unwrap());
    assert_eq!(poll(&core, &t, 905, true, Some("original"), &observation("2")).status, RebootRequired);
    assert_eq!(poll(&core, &t, 915, true, Some("restarted"), &observation("2")).status, Verified);
}

#[test]
fn exit_cancellation_error_and_reboot_are_distinct() {
    for (code, status) in [(1223, Cancelled), (1602, Cancelled), (1603, Failed), (1641, RebootRequired), (3010, RebootRequired)] {
        let (_d, core, p, d) = fixture();
        let t = task(&core, &p, &d, observation("1"), true);
        assert!(core.store.record_installer_exit(&t.check.id, ProcessExit::Code(code), Some("old-boot".into())).unwrap());
        assert_eq!(poll(&core, &t, 8, true, Some("old-boot"), &observation("2")).status, status);
        expect_version(&core, &p, "v1.0");
        assert_eq!(core.store.download(&d.id).unwrap().status, "completed");
        let result = poll(&core, &t, 13, true, Some("new-boot"), &observation("2"));
        if status == RebootRequired { assert_eq!(result.status, Verified); }
        else { assert_eq!(result.status, status); expect_version(&core, &p, "v1.0"); }
    }
}

#[test]
fn rejected_launch_is_terminal_and_does_not_report_an_install() {
    let (_d, core, p, d) = fixture();
    for (cancelled, expected) in [(true, Cancelled), (false, Failed)] {
        let t = core.prepare_installation_check(&p, &d, observation("1"));
        core.store.begin_installation_check(&t, None).unwrap();
        core.store.fail_installer_launch(&t.check.id, cancelled, "fixture launch not accepted").unwrap();
        assert_eq!(poll(&core, &t, 20, true, None, &observation("2")).status, expected);
        assert!(core.confirm_installation(&t.check.id).is_err());
        expect_version(&core, &p, "v1.0");
    }
}

#[test]
fn version_matching_preserves_preview_and_build_and_rejects_unknown_data() {
    assert_eq!(version(" v1.2 "), version("V1.2.0.0"));
    assert_ne!(version("1.2.0-rc.1"), version("1.2.0"));
    assert_ne!(version("1.2.0+one"), version("1.2.0+two"));
    assert!(version("2.0 release").is_none());
    assert!(version("2,0").is_none());
    assert!(newer("1.2.0", "1.2.0-rc.1"));
    assert!(!newer("1.2.0-rc.1", "1.2.0"));
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), false);
    assert_eq!(poll(&core, &t, 5, false, None, &observation("2.0-rc.1")).status, Detecting);
    let already = task(&core, &p, &d, observation("2"), false);
    assert_eq!(poll(&core, &already, 5, false, None, &observation("2")).status, Detecting);
    expect_version(&core, &p, "v1.0");
}

#[test]
fn ambiguous_and_incomplete_observations_cannot_match() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), false);
    let mut seen = observation("2");
    seen.observations.push(Observation { identity: identity("HKLM\\same-name"), version: Some("2".into()) });
    assert_eq!(poll(&core, &t, 5, false, None, &seen).status, Detecting);
    seen.observations.pop(); seen.registry_complete = false;
    assert_eq!(poll(&core, &t, 10, false, None, &seen).status, Detecting);
    expect_version(&core, &p, "v1.0");
}

#[test]
fn stale_probe_does_not_override_a_new_task_edit_deletion_or_file_mode() {
    for scenario in ["newer task", "manual edit", "project gone", "download gone", "files", "manual confirm"] {
        let (_d, core, p, d) = fixture();
        let t = task(&core, &p, &d, observation("1"), false);
        core.verify_installations_with(Some(&p.id), false, None, since(&t, 8), None, |_, _| {
            match scenario {
                "newer task" => { task(&core, &p, &d, observation("1"), false); }
                "manual edit" => { core.update_project(&p.id, super::super::decode(json!({"installedVersion": "v3"})).unwrap()).unwrap(); }
                "project gone" => { core.store.remove_project(&p.id).unwrap(); }
                "download gone" => { core.delete_download(&d.id).unwrap(); }
                "files" => { core.update_project(&p.id, super::super::decode(json!({"managementMode": "files"})).unwrap()).unwrap(); }
                "manual confirm" => { core.confirm_installation(&t.check.id).unwrap(); }
                _ => unreachable!(),
            }
            observation("2")
        }).unwrap();
        match scenario {
            "project gone" => assert!(core.store.project(&p.id).is_err()),
            "manual confirm" => {
                assert_eq!(core.store.installation_record(&t.check.id).unwrap().check.status, Manual);
                assert_eq!(core.store.project(&p.id).unwrap().installed_source.as_deref(), Some("manual"));
            }
            _ => {
                assert_eq!(core.store.installation_record(&t.check.id).unwrap().check.status, Superseded);
                match scenario {
                    "manual edit" => expect_version(&core, &p, "v3"),
                    "files" => assert_eq!(core.store.project(&p.id).unwrap().management_mode, "files"),
                    _ => expect_version(&core, &p, "v1.0"),
                }
            }
        }
        if scenario == "download gone" { assert!(core.store.optional_download(&d.id).unwrap().is_none()); }
        assert!(core.store.install_binding(&p.id).unwrap().is_none(), "{scenario}");
    }
}

#[test]
fn persisted_task_resumes_after_restart_without_the_process_handle() {
    let (dir, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), true);
    drop(core);
    let reopened = new_core(dir.path());
    assert!(!reopened.store.installation_record(&t.check.id).unwrap().exit_pending);
    assert_eq!(poll(&reopened, &t, 20, true, None, &observation("2")).status, Verified);
    expect_version(&reopened, &p, "v2.0");
}

#[test]
fn crash_between_baseline_and_launch_does_not_assume_the_installer_opened() {
    let (dir, core, p, d) = fixture();
    let t = core.prepare_installation_check(&p, &d, observation("1"));
    core.store.begin_installation_check(&t, None).unwrap();
    drop(core);
    let reopened = new_core(dir.path());
    assert!(!reopened.store.installation_record(&t.check.id).unwrap().launched);
    assert_eq!(poll(&reopened, &t, 15, true, None, &observation("2")).status, Unconfirmed);
    expect_version(&reopened, &p, "v1.0");
    assert_eq!(reopened.store.download(&d.id).unwrap().status, "completed");
    reopened.confirm_installation(&t.check.id).unwrap();
    expect_version(&reopened, &p, "v2.0");
}

#[test]
fn no_detector_never_changes_the_version_and_respects_schedule() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, Probe::default(), false);
    let mut calls = Vec::new();
    for (second, force, expected) in [(0,false,Detecting), (4,false,Detecting), (5,false,Detecting),
        (119,false,Detecting), (120,false,Detecting), (134,false,Detecting), (900,false,Unconfirmed),
        (910,false,Unconfirmed), (911,true,Unconfirmed)] {
        let checks = core.verify_installations_with(None, force, None, since(&t, second), None, |_, _| {
            calls.push(second); Probe::default()
        }).unwrap();
        assert_eq!(checks[0].status, expected);
    }
    assert_eq!(calls, vec![0,5,119,134,900,911]);
    expect_version(&core, &p, "v1.0");
    assert_eq!(core.store.download(&d.id).unwrap().status, "completed");
    core.confirm_installation(&t.check.id).unwrap();
    expect_version(&core, &p, "v2.0");
}

#[test]
fn bindings_sync_slowly_and_never_downgrade_or_override_a_manual_edit() {
    let (_d, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), false);
    assert_eq!(poll(&core, &t, 5, false, None, &observation("2")).status, Verified);
    let binding = core.store.install_binding(&p.id).unwrap().unwrap();
    let at = DateTime::parse_from_rfc3339(&binding.checked_at).unwrap().with_timezone(&Utc);
    let run = |seconds, value| core.verify_installations_with(None, true, None, at + Duration::seconds(seconds), None, |_, ids| {
        assert_eq!(ids, binding.identities.as_slice()); observation(value)
    }).unwrap();
    run(1799, "3"); expect_version(&core, &p, "v2.0");
    run(1800, "1"); expect_version(&core, &p, "v2.0");
    run(3599, "3"); expect_version(&core, &p, "v2.0");
    core.verify_installations_with(None, true, None, at + Duration::seconds(3600), None, |_, _| {
        let mut unrelated = observation("9"); unrelated.observations[0].identity = identity("HKLM\\new fixture identity"); unrelated
    }).unwrap();
    expect_version(&core, &p, "v2.0");
    run(5400, "3"); expect_version(&core, &p, "3");
    core.update_project(&p.id, super::super::decode(json!({"installedVersion":"v4"})).unwrap()).unwrap();
    run(7200, "5"); expect_version(&core, &p, "v4");
}

#[test]
fn verification_and_manual_confirmation_roll_back_on_task_write_failure() {
    let (dir, core, p, d) = fixture();
    let t = task(&core, &p, &d, observation("1"), false);
    let db = rusqlite::Connection::open(dir.path().join("releasehub.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_task BEFORE UPDATE ON installation_checks BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
    assert!(core.verify_installations_with(None, false, None, since(&t, 3), None, |_, _| observation("2")).is_err());
    assert!(core.confirm_installation(&t.check.id).is_err());
    expect_version(&core, &p, "v1.0");
    assert_eq!(core.store.download(&d.id).unwrap().status, "completed");
    assert_eq!(core.store.installation_record(&t.check.id).unwrap(), t);
    assert!(core.store.install_binding(&p.id).unwrap().is_none());
}
