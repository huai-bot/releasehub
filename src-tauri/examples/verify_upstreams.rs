//! Explicit development check; not included in the desktop distribution.
//! Build without credentials, then optionally supply RELEASEHUB_TEST_GITHUB_TOKEN
//! only to this executable's process. It never opens an upstream executable.
use releasehub_lib::core::Core;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::main]
async fn main() {
    if let Err(error) = verify().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn verify() -> Result<(), String> {
    let directory = std::env::args().nth(1).map(PathBuf::from).ok_or("An absolute, absent test directory is required")?;
    if !directory.is_absolute() || directory.exists() {
        return Err("Use a new absolute test directory, never an existing application directory".into());
    }
    let core = Core::new(directory.clone(), true, Arc::new(|_, _| {}))?;
    core.set_github_token(std::env::var("RELEASEHUB_TEST_GITHUB_TOKEN").ok());
    core.handle("save_settings", json!({"settings":{"checkIntervalMinutes":0,"notifications":false,"autoDownload":false}})).await?;
    for repository in ["Lake1059/FFmpegFreeUI", "RoutineMiku/apkstart", "Predidit/Kazumi"] {
        core.handle("add_project", json!({"input":{"repository":repository,"channel":"stable","preference":"portable"}})).await?;
    }
    let checks = core.handle("check_updates", json!({"allowAutoDownload":false})).await?;
    if checks["errors"].as_u64() != Some(0) { return Err("An upstream update check failed".into()); }
    let snapshot = core.handle("snapshot", json!({})).await?;
    let projects = snapshot["projects"].as_array().ok_or("Missing project results")?;
    if projects.len() != 3 { return Err("Expected three checked projects".into()); }
    let mut summary = Vec::new();
    for project in projects {
        let repo = project["repo"].as_str().ok_or("Missing repository")?;
        let selected = project["selectedAssetId"].as_u64().ok_or_else(|| format!("No selected asset for {repo}"))?;
        let asset = project["release"]["assets"].as_array().and_then(|assets| assets.iter().find(|asset| asset["id"].as_u64() == Some(selected))).ok_or("Selected asset not found")?;
        let name = asset["name"].as_str().ok_or("Missing asset filename")?;
        let expected = match repo.to_ascii_lowercase().as_str() {
            "ffmpegfreeui" => name.eq_ignore_ascii_case("FFmpegFreeUI.x64.exe"),
            "apkstart" => name.to_ascii_lowercase().contains("win64-portable.zip"),
            "kazumi" => name.to_ascii_lowercase().starts_with("kazumi_windows_") && name.ends_with(".zip"),
            _ => false,
        };
        if !expected || asset["compatible"] != true { return Err(format!("Unexpected Windows package for {repo}: {name}")); }
        summary.push(json!({"repository":project["url"],"tag":project["release"]["tag"],"selectedAsset":name,"size":asset["size"]}));
    }
    // Exercise one real upstream-digest download and safe managed extraction.
    // Kazumi's small Windows ZIP is prepared but its program is never launched.
    let project = projects.iter().find(|project| project["repo"] == "Kazumi").ok_or("Kazumi not found")?;
    let queued = core.handle("download_asset", json!({"projectId":project["id"],"assetId":project["selectedAssetId"]})).await?;
    let download_id = queued["id"].as_str().ok_or("Missing download ID")?.to_owned();
    let completed = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let current = core.handle("snapshot", json!({})).await?;
            let download = current["downloads"].as_array().and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(&download_id))).ok_or("Download record disappeared")?;
            match download["status"].as_str() {
                Some("completed") => return Ok::<Value, String>(download.clone()),
                Some("failed" | "cancelled") => return Err(download["error"].as_str().unwrap_or("Download failed").to_owned()),
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }).await.map_err(|_| "Live download check timed out")??;
    if completed["verified"] != true { return Err("The actual download did not match an upstream SHA-256 digest".into()); }
    let rejected = core.handle("install_download", json!({"id":download_id,"confirmed":false})).await;
    if rejected.is_ok() { return Err("Install did not require confirmation".into()); }
    let installed = core.handle("install_download", json!({"id":download_id,"confirmed":true})).await?;
    if installed["kind"] != "portable" || installed["installed"] != true { return Err("Managed ZIP preparation failed".into()); }
    let installed_path = PathBuf::from(installed["path"].as_str().ok_or("Missing managed path")?);
    if !installed_path.starts_with(directory.canonicalize().map_err(|_| "Test directory unavailable")?) {
        return Err("Managed program escaped the isolated test directory".into());
    }
    // Agent work is coordinated without executing source or dispatching remote builds.
    let task = core.handle("create_build_task", json!({"projectId":project["id"],"instructions":"Development verification only. Do not build or execute source."})).await?;
    if task["sourceCommit"].as_str().map_or(true, |sha| sha.len() != 40) { return Err("Build task did not lock a full commit".into()); }
    core.handle("cancel_build_task", json!({"id":task["id"]})).await?;
    core.set_github_token(None);
    println!("{}", serde_json::to_string_pretty(&json!({"ok":true,"projects":summary,"downloadVerified":true,"managedZipPrepared":true,"upstreamExecuted":false,"agentTaskPinnedAndCancelled":true})).map_err(|_| "Cannot format summary")?);
    Ok(())
}
