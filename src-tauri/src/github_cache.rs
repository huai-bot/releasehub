//! Machine-local, versioned projection cache. Never stores OAuth material.
//! Every clear changes an epoch before deleting files, fencing in-flight writes.
use crate::github::{check_regular_file, reject_link_ancestors, validate_auth_root};
use crate::github_oauth::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::PathBuf, sync::Mutex};

const VERSION: u8 = 1;
// The separate official-feed cache reserves the remaining 2 MiB.
const MAX_BYTES: u64 = 98 * 1024 * 1024;
const MAX_ENTRY: u64 = 12 * 1024 * 1024;
const MAX_AGE: u64 = 30 * 24 * 60 * 60 * 1000;
#[derive(Serialize, Deserialize)]
struct Entry { version: u8, namespace: String, operation: String, parameters: Value, updated_at: u64, data: Value }
struct State { epoch: u64 }
pub(crate) struct GithubCache { root: PathBuf, state: Mutex<State>, enabled: bool }
impl GithubCache {
    pub fn new(auth_root: PathBuf, enabled: bool) -> Self {
        Self { root: auth_root.join("github-cache-v1"), state: Mutex::new(State { epoch: 0 }), enabled }
    }
    pub fn epoch(&self) -> u64 { self.state.lock().unwrap_or_else(|e| e.into_inner()).epoch }
    pub fn key(namespace: &str, operation: &str, parameters: &Value) -> String {
        let bytes = serde_json::to_vec(&json!([VERSION, namespace, operation, parameters])).unwrap_or_default();
        format!("{:x}", Sha256::digest(bytes))
    }
    fn safe_root(&self, create: bool) -> Result<bool, String> {
        if !self.enabled { return Ok(false); }
        validate_auth_root(&self.root)?;
        reject_link_ancestors(&self.root)?;
        if create { fs::create_dir_all(&self.root).map_err(|_| "无法创建本机 GitHub 缓存目录。")?; }
        Ok(self.root.is_dir())
    }
    pub fn read(&self, namespace: &str, operation: &str, parameters: &Value, revision: u64, ttl: u64) -> Result<Value, String> {
        let _guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !self.safe_root(false)? { return Ok(Value::Null); }
        let path = self.root.join(format!("{}.json", Self::key(namespace, operation, parameters)));
        check_regular_file(&path)?;
        let file = match fs::File::open(&path) { Ok(file) => file, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null), Err(_) => return Err("无法读取本机 GitHub 缓存。".into()) };
        let mut bytes = Vec::new();
        file.take(MAX_ENTRY + 1).read_to_end(&mut bytes).map_err(|_| "无法读取本机 GitHub 缓存。")?;
        if bytes.len() as u64 > MAX_ENTRY { return Ok(Value::Null); }
        let entry: Entry = match serde_json::from_slice(&bytes) { Ok(entry) => entry, Err(_) => return Ok(Value::Null) };
        let now = now_ms();
        if entry.version != VERSION || entry.namespace != namespace || entry.operation != operation || entry.parameters != *parameters || entry.updated_at > now || now.saturating_sub(entry.updated_at) > MAX_AGE { return Ok(Value::Null); }
        Ok(json!({ "data": entry.data, "updatedAt": entry.updated_at, "fresh": now.saturating_sub(entry.updated_at) <= ttl, "sessionRevision": revision }))
    }
    pub fn put(&self, epoch: u64, namespace: &str, operation: &str, parameters: &Value, data: &Value) -> Result<(), String> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch != epoch || !self.safe_root(true)? { return Ok(()); }
        let entry = Entry { version: VERSION, namespace: namespace.into(), operation: operation.into(), parameters: parameters.clone(), updated_at: now_ms(), data: data.clone() };
        let bytes = serde_json::to_vec(&entry).map_err(|_| "无法生成 GitHub 缓存。")?;
        if bytes.len() as u64 > MAX_ENTRY { return Ok(()); }
        let path = self.root.join(format!("{}.json", Self::key(namespace, operation, parameters)));
        let temporary = self.root.join(format!("{}.tmp", Self::key(namespace, operation, parameters)));
        check_regular_file(&path)?; check_regular_file(&temporary)?;
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&temporary).map_err(|_| "无法写入本机 GitHub 缓存。")?;
        file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| "无法写入本机 GitHub 缓存。")?;
        drop(file);
        // Both targets are exact hash-named regular files under validated root.
        // On Windows rename does not replace an existing file.
        if path.exists() { fs::remove_file(&path).map_err(|_| "无法更新本机 GitHub 缓存。")?; }
        fs::rename(&temporary, &path).map_err(|_| "无法保存本机 GitHub 缓存。")?;
        self.prune()?;
        Ok(())
    }
    fn files(&self) -> Result<Vec<(PathBuf, u64, u64)>, String> {
        if !self.safe_root(false)? { return Ok(Vec::new()); }
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.root).map_err(|_| "无法检查 GitHub 缓存。")? {
            let item = item.map_err(|_| "无法检查 GitHub 缓存。")?;
            let path = item.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue; };
            let Some(stem) = name.strip_suffix(".json").or_else(|| name.strip_suffix(".tmp")) else { continue; };
            if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) { continue; }
            check_regular_file(&path)?;
            let metadata = fs::metadata(&path).map_err(|_| "无法检查 GitHub 缓存。")?;
            let time = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |t| t.as_millis() as u64);
            entries.push((path, metadata.len(), time));
        }
        Ok(entries)
    }
    fn prune(&self) -> Result<(), String> {
        let mut files = self.files()?;
        files.sort_by_key(|(_, _, time)| *time);
        let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
        let now = now_ms();
        for (path, size, time) in files {
            if total > MAX_BYTES || now.saturating_sub(time) > MAX_AGE || size > MAX_ENTRY || path.extension().is_some_and(|s| s == "tmp") {
                // files() checked the exact path is a regular cache-owned file.
                check_regular_file(&path)?;
                fs::remove_file(&path).map_err(|_| "无法清理过期 GitHub 缓存。")?;
                total = total.saturating_sub(size);
            }
        }
        Ok(())
    }
    pub fn stats(&self) -> Result<Value, String> {
        let _guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.prune()?;
        let files = self.files()?;
        Ok(json!({"bytes": files.iter().map(|(_, n, _)| n).sum::<u64>(), "entries": files.len()}))
    }
    pub fn invalidate(&self, namespace: &str, operations: &[&str], repositories: Option<&[String]>) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = state.epoch.wrapping_add(1);
        for (path, size, _) in self.files()? {
            if size > MAX_ENTRY { continue; }
            let Some(entry) = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok()) else { continue; };
            if entry.namespace != namespace || !operations.contains(&entry.operation.as_str()) { continue; }
            let selected = repositories.is_none_or(|names| {
                !matches!(entry.operation.as_str(), "github_repository" | "github_repository_lists")
                    || entry.parameters["repository"].as_str().is_some_and(|repository| names.iter().any(|name| name.eq_ignore_ascii_case(repository)))
            });
            if selected {
                check_regular_file(&path)?;
                fs::remove_file(path).map_err(|_| "无法更新本机 GitHub 缓存。")?;
            }
        }
        Ok(())
    }
    pub fn clear_protected(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = state.epoch.wrapping_add(1);
        for (path, size, _) in self.files()? {
            let guest = size <= MAX_ENTRY && fs::read(&path).ok()
                .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
                .is_some_and(|entry| entry.version == VERSION && entry.namespace == "guest");
            if !guest {
                check_regular_file(&path)?;
                fs::remove_file(path).map_err(|_| "无法清理本机 GitHub 账号缓存。")?;
            }
        }
        Ok(())
    }
    pub fn clear(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = state.epoch.wrapping_add(1);
        for (path, _, _) in self.files()? {
            check_regular_file(&path)?;
            fs::remove_file(path).map_err(|_| "无法清理本机 GitHub 缓存。")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_roundtrip_is_account_scope_parameter_and_version_isolated() {
        let temp = tempfile::tempdir().unwrap(); let cache = GithubCache::new(temp.path().join("local"), true);
        let args = json!({"page":1}); let epoch = cache.epoch();
        cache.put(epoch, "42:repo", "github_repositories", &args, &json!({"items":[]})).unwrap();
        let entry = cache.read("42:repo", "github_repositories", &args, 7, 300_000).unwrap();
        assert_eq!(entry["sessionRevision"], 7); assert_eq!(entry["fresh"], true);
        for ns in ["43:repo", "42:public_repo", "guest"] { assert!(cache.read(ns, "github_repositories", &args, 7, 300_000).unwrap().is_null()); }
        assert!(cache.read("42:repo", "github_repositories", &json!({"page":2}), 7, 300_000).unwrap().is_null());
        let reopened = GithubCache::new(temp.path().join("local"), true);
        assert_eq!(reopened.read("42:repo", "github_repositories", &args, 9, 300_000).unwrap()["data"], entry["data"]);
        assert_eq!(reopened.stats().unwrap()["entries"], 1);
    }
    #[test]
    fn clear_fences_late_response_and_never_removes_unowned_file() {
        let temp = tempfile::tempdir().unwrap(); let cache = GithubCache::new(temp.path().join("local"), true);
        let epoch = cache.epoch(); let args = json!({});
        cache.put(epoch, "guest", "github_repository", &args, &json!({})).unwrap();
        fs::write(cache.root.join("not-a-cache.json"), "unrelated").unwrap();
        cache.clear().unwrap();
        cache.put(epoch, "guest", "github_repository", &args, &json!({})).unwrap();
        assert_eq!(cache.stats().unwrap()["entries"], 0);
        assert!(cache.root.join("not-a-cache.json").exists());
        cache.put(cache.epoch(), "guest", "github_repository", &args, &json!({})).unwrap();
        assert_eq!(cache.stats().unwrap()["entries"], 1);
    }
    #[test]
    fn mutation_invalidation_keeps_unrelated_details_activity_and_accounts() {
        let temp = tempfile::tempdir().unwrap(); let cache = GithubCache::new(temp.path().join("local"), true);
        let epoch = cache.epoch(); let first = json!({"repository":"octocat/one"}); let other = json!({"repository":"octocat/two"});
        for args in [&first, &other] { cache.put(epoch, "42:repo", "github_repository", args, &json!({})).unwrap(); }
        cache.put(epoch, "42:repo", "github_activity", &json!({"page":1}), &json!([])).unwrap();
        cache.put(epoch, "43:repo", "github_repository", &first, &json!({})).unwrap();
        cache.invalidate("42:repo", &["github_repository"], Some(&["octocat/one".into()])).unwrap();
        assert!(cache.read("42:repo", "github_repository", &first, 0, 10).unwrap().is_null());
        assert!(!cache.read("42:repo", "github_repository", &other, 0, 10).unwrap().is_null());
        assert!(!cache.read("43:repo", "github_repository", &first, 0, 10).unwrap().is_null());
        assert!(!cache.read("42:repo", "github_activity", &json!({"page":1}), 0, 10).unwrap().is_null());
        cache.put(epoch, "42:repo", "github_repository", &first, &json!({})).unwrap();
        assert!(cache.read("42:repo", "github_repository", &first, 0, 10).unwrap().is_null());
    }
    #[test]
    fn missing_authority_cleans_account_cache_but_keeps_guest_restart_cache() {
        let temp = tempfile::tempdir().unwrap(); let cache = GithubCache::new(temp.path().join("local"), true);
        let args = json!({}); let epoch = cache.epoch();
        for namespace in ["guest", "42:repo"] { cache.put(epoch, namespace, "github_repository", &args, &json!({})).unwrap(); }
        cache.clear_protected().unwrap();
        assert!(!cache.read("guest", "github_repository", &args, 0, 300_000).unwrap().is_null());
        assert!(cache.read("42:repo", "github_repository", &args, 0, 300_000).unwrap().is_null());
        cache.put(epoch, "42:repo", "github_repository", &args, &json!({})).unwrap();
        assert_eq!(cache.stats().unwrap()["entries"], 1);
    }
    #[test]
    fn malformed_entry_and_future_timestamp_are_cache_misses() {
        let temp = tempfile::tempdir().unwrap(); let cache = GithubCache::new(temp.path().join("local"), true);
        cache.safe_root(true).unwrap(); let args = json!({});
        let path = cache.root.join(format!("{}.json", GithubCache::key("guest", "github_lists", &args)));
        fs::write(&path, "not json").unwrap();
        assert!(cache.read("guest", "github_lists", &args, 0, 10).unwrap().is_null());
        let entry = Entry {version: VERSION, namespace:"guest".into(),operation:"github_lists".into(),parameters:args.clone(),updated_at:now_ms()+100000,data:json!([])};
        fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(cache.read("guest", "github_lists", &args, 0, 10).unwrap().is_null());
    }
}
