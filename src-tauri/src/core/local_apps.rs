//! Record existing executables and conservatively resolve their public update source.
use super::{library, models::*, network, windows_probe::{self, LocalAppMetadata}, Core};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::{fs::{self, File}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}, sync::{atomic::Ordering, Arc}, time::{Duration, SystemTime}};
use url::Url;

const MAX_DROP: usize = 32;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalAddResult {
    added: Vec<Project>,
    skipped: Vec<LocalAddSkipped>,
}

#[derive(Serialize)]
struct LocalAddSkipped { path: String, reason: String }

pub(super) fn has_repository(p: &Project) -> bool { !p.owner.is_empty() && !p.repo.is_empty() }

pub(super) fn same_repository(a: &Project, b: &Project) -> bool {
    a.owner.eq_ignore_ascii_case(&b.owner) && a.repo.eq_ignore_ascii_case(&b.repo)
}

pub(super) fn executable_name(path: &Path) -> Option<String> {
    let name = path.file_stem()?.to_str()?.trim();
    (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.chars().take(200).collect())
}

pub(super) fn same_executable_path(a: &str, b: &str) -> bool {
    #[cfg(windows)]
    {
        fn key(path: &str) -> String { path.strip_prefix(r"\\?\").unwrap_or(path).replace('/', "\\").to_lowercase() }
        key(a) == key(b)
    }
    #[cfg(not(windows))]
    { Path::new(a) == Path::new(b) }
}

fn pending_executable(p: &Project) -> bool {
    !has_repository(p) && p.management_mode == "software" && p.repository_match.as_deref() == Some("pending") && p.executable_path.is_some()
}

pub(super) fn same_pending_executable(a: &Project, b: &Project) -> bool {
    pending_executable(a) && a.executable_path == b.executable_path
}

fn read_executable(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let mut file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || super::update::linked(&meta) { return None; }
    let mut dos = [0; 64];
    file.read_exact(&mut dos).ok()?;
    if dos[..2] != *b"MZ" { return None; }
    let pe = u32::from_le_bytes(dos[60..64].try_into().ok()?) as u64;
    if !(64..=16 * 1024 * 1024).contains(&pe) || pe + 26 > meta.len() { return None; }
    file.seek(SeekFrom::Start(pe)).ok()?;
    let mut header = [0; 26];
    file.read_exact(&mut header).ok()?;
    let flags = u16::from_le_bytes([header[22], header[23]]);
    let optional_size = u16::from_le_bytes([header[20], header[21]]) as u64;
    let magic = u16::from_le_bytes([header[24], header[25]]);
    (header[..4] == *b"PE\0\0" && flags & 0x2002 == 2 && optional_size >= 2 && pe + 24 + optional_size <= meta.len()
        && matches!(magic, 0x10b | 0x20b)).then(|| (meta.len(), meta.modified().ok()))
}

fn github_hint(s: &str) -> Option<(String, String)> {
    if s.len() > 2048 || s.chars().any(char::is_control) { return None; }
    let s = s.trim();
    let url = Url::parse(s).ok()?;
    // Do not follow arbitrary URLs recorded by executables; use the GitHub API by validated name only.
    if !s.get(..8)?.eq_ignore_ascii_case("https://") || url.scheme() != "https" || url.host_str() != Some("github.com")
        || s[8..].split('/').next()?.contains('@') || url.port().is_some() || url.query().is_some() || url.fragment().is_some()
        || s.contains(['\\', '%']) || s[8..].split('/').any(|part| matches!(part, "." | "..")) { return None; }
    let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    let (owner, repo) = match parts.as_slice() {
        ["", owner, repo] | ["", owner, repo, "releases"] | ["", owner, repo, "releases", "latest"] => (owner, repo),
        ["", owner, repo, "releases", "tag", tag] if !tag.is_empty() => (owner, repo),
        _ => return None,
    };
    network::repository(&format!("{owner}/{repo}")).ok()
}

fn explicit_repository(hints: &[String]) -> Result<Option<(String, String)>, ()> {
    let mut repos = hints.iter().filter_map(|hint| github_hint(hint));
    let Some((owner, repo)) = repos.next() else { return Ok(None); };
    if repos.any(|(o, r)| !owner.eq_ignore_ascii_case(&o) || !repo.eq_ignore_ascii_case(&r)) { return Err(()); }
    Ok(Some((owner, repo)))
}

fn name_key(name: &str) -> Option<String> {
    if name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_' | b'.')) { return None; }
    let key: String = name.bytes().filter(|b| b.is_ascii_alphanumeric()).map(|b| b.to_ascii_lowercase() as char).collect();
    (key.len() >= 3).then_some(key)
}

fn search_identity(meta: &LocalAppMetadata) -> Option<(String, String, &str)> {
    let product = name_key(meta.product.as_deref()?)?;
    let company = meta.publisher.as_deref()?;
    name_key(company)?;
    let (publisher, _) = network::repository(&format!("{company}/placeholder")).ok()?;
    let filename = meta.original_filename.as_deref()?;
    let basename = filename.get(..filename.len().checked_sub(4)?)?;
    // Reject generic or unrelated filenames and never send an EXE path to GitHub.
    if !filename.get(basename.len()..)?.eq_ignore_ascii_case(".exe") || name_key(basename)? != product
        || matches!(product.as_str(), "app" | "application" | "setup" | "install" | "installer" | "update" | "updater" | "program" | "tool") { return None; }
    Some((product, publisher, filename))
}

fn unique_search_repository(raw: &Value, product: &str, publisher: &str) -> Option<(String, String)> {
    let items = raw.get("items")?.as_array()?;
    if raw.get("incomplete_results")?.as_bool()? || raw.get("total_count")?.as_u64()? != items.len() as u64 || items.len() > 10 { return None; }
    let mut matches = items.iter().filter_map(|item| {
        if item.get("private").and_then(Value::as_bool) != Some(false)
            || item.get("fork").and_then(Value::as_bool) != Some(false) { return None; }
        let (owner, repo) = network::repository(item.get("full_name")?.as_str()?).ok()?;
        (name_key(&repo)?.as_str() == product && owner.eq_ignore_ascii_case(publisher)).then_some((owner, repo))
    });
    let unique = matches.next()?;
    matches.next().is_none().then_some(unique)
}

impl Core {
    pub(crate) async fn add_local_executables(self: &Arc<Self>, paths: Vec<PathBuf>) -> Result<LocalAddResult, String> {
        if paths.len() > MAX_DROP { return Err(format!("一次最多添加 {MAX_DROP} 个 EXE")); }
        let core = self.clone();
        let result = tokio::task::spawn_blocking(move || core.record_local_executables(paths)).await.map_err(|_| "无法读取选中的程序")??;
        if !result.added.is_empty() { self.changed(); self.resume_local_repository_matches(); }
        Ok(result)
    }

    fn record_local_executables(&self, paths: Vec<PathBuf>) -> Result<LocalAddResult, String> {
        let mut result = LocalAddResult { added: Vec::new(), skipped: Vec::new() };
        for path in paths {
            let outcome = (|| {
                let name = executable_name(&path).ok_or("程序文件名无效")?;
                let text = path.to_str().filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control)).ok_or("程序路径无效或过长")?;
                if !windows_probe::local_executable_path(&path) { return Err("请选择本机磁盘的 EXE 程序".into()); }
                let mut p = library::empty_project(String::new(), String::new(), "stable".into(), "auto".into());
                let (root, exe) = self.resolve_executable_selection(&p, text)?;
                if !windows_probe::local_executable_path(&exe) || read_executable(&exe).is_none() { return Err("此文件不是可识别的本机 EXE 程序".into()); }
                p.name = name;
                p.installed_path = Some(root.to_str().ok_or("程序目录无效")?.into());
                p.executable_path = Some(exe.to_str().ok_or("程序路径无效")?.into());
                p.installed_version = windows_probe::local_app_version(&exe);
                if p.installed_version.is_some() { p.installed_source = Some("executable".into()); }
                p.repository_match = Some("pending".into());
                Ok::<_, String>(p)
            })();
            match outcome {
                Ok(p) if self.store.insert_local_project(&p)? => result.added.push(p),
                Ok(_) => result.skipped.push(LocalAddSkipped { path: path.display().to_string(), reason: "此程序已在软件库中".into() }),
                Err(reason) => result.skipped.push(LocalAddSkipped { path: path.display().to_string(), reason }),
            }
        }
        Ok(result)
    }

    pub(crate) fn resume_local_repository_matches(self: &Arc<Self>) {
        let core = self.clone();
        tokio::spawn(async move {
            let _worker = core.local_repository_worker.lock().await;
            let Ok(mut projects) = core.store.projects() else { return; };
            projects.reverse();
            for p in projects.into_iter().filter(pending_executable) {
                if let Ok(Some(linked)) = core.match_local_repository(p).await {
                    let _ = core.check_project_after_repository_change(&linked).await;
                }
            }
        });
    }

    async fn match_local_repository(self: &Arc<Self>, p: Project) -> Result<Option<Project>, String> {
        // Read and stat the selected file on a blocking thread, then only send bounded public lookup terms.
        if !self.store.project(&p.id).is_ok_and(|current| same_pending_executable(&current, &p)) { return Ok(None); }
        let core = self.clone();
        let previous = p.clone();
        let inspected = tokio::task::spawn_blocking(move || {
            let path = previous.executable_path.as_deref()?;
            if !windows_probe::local_executable_path(Path::new(path)) { return None; }
            let path = core.validate_executable(&previous, path).ok()?;
            let stamp = read_executable(&path)?;
            let metadata = windows_probe::local_app_metadata(&path);
            Some((path, stamp, metadata))
        }).await.ok().flatten();
        let (matched, version) = if let Some((path, stamp, metadata)) = inspected {
            let matched = match explicit_repository(&metadata.repository_urls) {
                Ok(Some((owner, repo))) => self.public_repository(owner, repo).await.ok(),
                Ok(None) => self.search_local_repository(&metadata).await,
                Err(()) => None, // Contradictory attribution rules out the heuristic too.
            };
            let core = self.clone();
            let previous = p.clone();
            let still_same = tokio::task::spawn_blocking(move || {
                previous.executable_path.as_deref().and_then(|text| core.validate_executable(&previous, text).ok())
                    .is_some_and(|current| current == path && read_executable(&current) == Some(stamp))
            }).await.unwrap_or(false);
            if still_same { (matched, metadata.version) } else { (None, None) }
        } else { (None, None) };
        if let Some((updated, _)) = self.store.bind_project_repository(&p.id, Some(&p), matched.as_ref(), version)? {
            self.changed();
            return Ok(has_repository(&updated).then_some(updated));
        }
        Ok(None)
    }

    async fn search_local_repository(&self, metadata: &LocalAppMetadata) -> Option<Project> {
        let (product, publisher, filename) = search_identity(metadata)?;
        let mut url = Url::parse("https://api.github.com/search/repositories").ok()?;
        let terms = metadata.product.as_deref()?;
        url.query_pairs_mut().append_pair("q", &format!("{terms} in:name user:{publisher}")).append_pair("per_page", "10");
        let response = self.proxy.client_builder().ok()?.user_agent(concat!("ReleaseHub/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(12))
            .build().ok()?.get(url).header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28").send().await.ok()?.error_for_status().ok()?;
        const MAX_SEARCH_BYTES: usize = 256 * 1024;
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(bytes) = stream.next().await {
            let bytes = bytes.ok()?;
            if body.len() + bytes.len() > MAX_SEARCH_BYTES { return None; }
            body.extend_from_slice(&bytes);
        }
        let (owner, repo) = unique_search_repository(&serde_json::from_slice(&body).ok()?, &product, &publisher)?;
        let candidate = self.public_repository(owner, repo).await.ok()?;
        let release = self.github.release(&candidate).await.ok()??;
        release.assets.iter().any(|asset| asset.downloadable && asset.name.eq_ignore_ascii_case(filename)).then_some(candidate)
    }

    async fn public_repository(&self, owner: String, repo: String) -> Result<Project, String> {
        let raw = self.github.api(&owner, &repo, &[], None).await?;
        let (canonical_owner, canonical_repo) = network::repository(raw.get("full_name").and_then(Value::as_str).ok_or("GitHub 仓库信息不完整")?)?;
        if !owner.eq_ignore_ascii_case(&canonical_owner) || !repo.eq_ignore_ascii_case(&canonical_repo) { return Err("GitHub 仓库地址已变化，请填写当前仓库地址".into()); }
        let mut p = library::empty_project(canonical_owner, canonical_repo, "stable".into(), "auto".into());
        library::metadata(&mut p, &raw)?;
        Ok(p)
    }

    pub(crate) async fn set_project_repository(self: &Arc<Self>, id: &str, repository: Option<&str>) -> Result<Project, String> {
        self.store.project(id)?;
        let matched = if let Some(text) = repository.map(str::trim).filter(|s| !s.is_empty()) {
            let (owner, repo) = network::repository(text)?;
            Some(self.public_repository(owner, repo).await?)
        } else { None };
        let (updated, detached) = {
            let _external = self.external_requests.lock().await;
            let installing = self.installing.lock().map_err(|_| "安装状态不可用")?;
            if installing.contains(id) || self.store.journals()?.iter().any(|j| j.backup.project_id == id) { return Err("正在安装或等待恢复，完成后才能更改仓库".into()); }
            let _queue = self.download_queue.lock().map_err(|_| "下载队列不可用")?;
            self.store.bind_project_repository(id, None, matched.as_ref(), None)?.ok_or("项目已删除")?
        };
        if !detached.is_empty() {
            if let Ok(active) = self.active_downloads.lock() {
                for id in &detached {
                    if let Some(flag) = active.get(id) { flag.store(true, Ordering::Release); }
                }
            }
            for id in detached {
                if let Ok(download) = self.store.download(&id) { self.download_event(&download); }
            }
        }
        self.changed();
        if has_repository(&updated) {
            let core = self.clone();
            let project = updated.clone();
            tokio::spawn(async move { let _ = core.check_project_after_repository_change(&project).await; });
        }
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // The loader recognizes these bounded headers, but there is no section or executable code.
    fn fixture_exe() -> [u8; 90] {
        let mut bytes = [0; 90];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&2u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes
    }

    fn core_fixture() -> (tempfile::TempDir, Arc<Core>, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let core = Core::new(root.join("data"), true, Arc::new(|_, _| {})).unwrap();
        let app = root.join("app");
        fs::create_dir(&app).unwrap();
        let executable = app.join("FixtureApp.EXE");
        fs::write(&executable, fixture_exe()).unwrap();
        (dir, core, executable)
    }

    fn download_fixture(p: &Project, status: &str, path: Option<String>) -> DownloadRecord {
        DownloadRecord {
            id: uuid::Uuid::new_v4().to_string(), project_id: p.id.clone(), project_name: p.name.clone(),
            asset_id: 42, file_name: "FixtureApp.zip".into(), tag: "v2.0.0".into(),
            url: "https://github.com/Fixture/Old/releases/download/v2.0.0/FixtureApp.zip".into(), kind: "portable".into(), installable: true,
            total_bytes: 13, received_bytes: 0, status: status.into(), path, sha256: None,
            verified: false, error: None, created_at: now(), updated_at: now(),
        }
    }

    #[test]
    fn recording_is_path_only_and_deduplicates_valid_local_exe() {
        let (_dir, core, executable) = core_fixture();
        let bogus = executable.with_file_name("Bogus.exe");
        let other = executable.with_file_name("FixtureApp.txt");
        fs::write(&bogus, b"MZ this is not a PE file").unwrap();
        fs::write(&other, fixture_exe()).unwrap();
        let result = core.record_local_executables(vec![executable.clone(), bogus, other, executable.clone()]).unwrap();
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.skipped.len(), 3);
        assert_eq!(core.store.projects().unwrap().len(), 1);
        assert_eq!(fs::read(&executable).unwrap(), fixture_exe());
        assert_eq!(fs::read_dir(core.data_dir.join("apps")).unwrap().count(), 0);
        let persisted = core.store.project(&result.added[0].id).unwrap();
        assert_eq!(persisted.status, "local");
        assert_eq!(persisted.repository_match.as_deref(), Some("pending"));
        assert!(persisted.owner.is_empty() && persisted.repo.is_empty() && persisted.url.is_empty());
        assert_eq!(persisted.executable_path.as_deref().map(Path::new), Some(executable.as_path()));
        assert!(persisted.installed_version.is_none());
        let json: Value = serde_json::from_str(&core.export_library().unwrap()).unwrap();
        assert!(json["projects"].as_array().unwrap().is_empty());
    }

    #[test]
    fn repository_evidence_rejects_lookalikes_conflicts_and_authorless_searches() {
        let correct = "https://github.com/Fixture/FixtureApp/releases";
        let alias = "https://github.com/fixture/fixtureapp/releases/tag/v2.0.0";
        assert_eq!(explicit_repository(&[correct.into(), alias.into()]), Ok(Some(("Fixture".into(), "FixtureApp".into()))));
        assert!(explicit_repository(&[correct.into(), "https://github.com/Other/FixtureApp".into()]).is_err());
        for source in ["https://github.com.example/Fixture/FixtureApp", "https://@github.com/Fixture/FixtureApp",
            "https://github.com/disguised/../Fixture/FixtureApp", "https://github.com/Fixture/FixtureApp/issues/123",
            "https://github.com/Fixture/FixtureApp?token=fixture", "https://github.com/Fixture/FixtureApp%2fnot-a-repository"] {
            assert!(github_hint(source).is_none(), "{source}");
        }
        let mut meta = LocalAppMetadata { product: Some("Fixture App".into()), original_filename: Some("FixtureApp.exe".into()), ..Default::default() };
        assert!(search_identity(&meta).is_none());
        meta.publisher = Some("Fixture".into());
        let (product, publisher, _) = search_identity(&meta).unwrap();
        let right = json!({"full_name":"Fixture/Fixture-App","private":false,"fork":false});
        let other = json!({"full_name":"Other/FixtureApp","private":false,"fork":false});
        let second = json!({"full_name":"Fixture/FixtureApp","private":false,"fork":false});
        let response = |items: Vec<Value>, incomplete| json!({"items":items,"total_count":items.len(),"incomplete_results":incomplete});
        assert!(unique_search_repository(&response(vec![other.clone()], false), &product, &publisher).is_none());
        assert!(unique_search_repository(&response(vec![right.clone(), second], false), &product, &publisher).is_none());
        assert!(unique_search_repository(&response(vec![right.clone()], true), &product, &publisher).is_none());
        assert_eq!(unique_search_repository(&response(vec![right, other], false), &product, &publisher), Some(("Fixture".into(), "Fixture-App".into())));
    }

    #[tokio::test]
    async fn manual_unlink_preempts_pending_match_and_linkless_rows_are_not_checked() {
        let (_dir, core, executable) = core_fixture();
        let old = core.record_local_executables(vec![executable]).unwrap().added.remove(0);
        let result = core.check_updates(&json!({"projectIds":[old.id]})).await.unwrap();
        assert_eq!(result["checked"], 0);
        assert_eq!(result["errors"], 0);
        let manual = core.set_project_repository(&old.id, None).await.unwrap();
        assert_eq!(manual.repository_match.as_deref(), Some("manual"));
        assert_eq!(manual.executable_path, old.executable_path);
        let candidate = library::empty_project("Fixture".into(), "Wrong".into(), "stable".into(), "auto".into());
        assert!(core.store.bind_project_repository(&old.id, Some(&old), Some(&candidate), Some("1.2.3".into())).unwrap().is_none());
        assert!(!has_repository(&core.store.project(&old.id).unwrap()));
    }

    #[tokio::test]
    async fn repository_switch_detaches_old_assets_and_keeps_local_installation() {
        let (_dir, core, executable) = core_fixture();
        let mut p = core.record_local_executables(vec![executable]).unwrap().added.remove(0);
        p.owner = "Fixture".into(); p.repo = "Old".into(); p.installed_version = Some("1.0.0".into());
        p.asset_pattern = Some("old".into()); p.checked_at = Some(now()); p.source_head_sha = Some("old".into());
        p.release = Some(Release { id: 1, tag: "v2.0.0".into(), name: String::new(), body: String::new(),
            published_at: now(), url: "https://github.com/Fixture/Old/releases/tag/v2.0.0".into(), prerelease: false, assets: vec![] });
        p.selected_asset_id = Some(42);
        core.store.put_project(&p).unwrap();
        assert_eq!(core.store.mark_updates_read(&[UpdateVersion { project_id: p.id.clone(), tag: "v2.0.0".into() }]).unwrap(), 1);
        let artifact = core.data_dir.join("fixture-old.zip");
        fs::write(&artifact, b"fixture bytes").unwrap();
        let completed = download_fixture(&p, "completed", Some(artifact.to_str().unwrap().into()));
        let external = download_fixture(&p, "external", None);
        core.store.put_download(&completed).unwrap();
        core.store.put_download(&external).unwrap();
        let replacement = library::empty_project("Fixture".into(), "Correct".into(), "stable".into(), "auto".into());
        let (updated, detached) = core.store.bind_project_repository(&p.id, None, Some(&replacement), None).unwrap().unwrap();
        assert_eq!(updated.repository_match.as_deref(), Some("manual"));
        assert_eq!(updated.repo, "Correct");
        assert_eq!(updated.installed_version, p.installed_version);
        assert_eq!(updated.executable_path, p.executable_path);
        assert!(updated.release.is_none() && updated.selected_asset_id.is_none() && updated.source_head_sha.is_none() && updated.checked_at.is_none() && updated.asset_pattern.is_none());
        assert_eq!(updated.status, "notChecked");
        assert!(detached.contains(&completed.id) && detached.contains(&external.id));
        let saved = core.store.download(&completed.id).unwrap();
        assert!(saved.project_id.is_empty() && !saved.installable);
        assert_eq!(saved.path, completed.path);
        assert_eq!(fs::read(artifact).unwrap(), b"fixture bytes");
        assert_eq!(core.store.download(&external.id).unwrap().status, "cancelled");
        assert!(core.retry_download(&external.id).is_err());
        assert!(core.retry_external_download(&external.id).await.is_err());
        assert!(core.preview_install(&completed.id, None).is_err());
        let mut refreshed = updated;
        refreshed.release = p.release;
        core.store.put_project(&refreshed).unwrap();
        assert!(core.store.current_read_tags().unwrap().is_empty(), "same tag in the new repository must not inherit read state");
    }
}
