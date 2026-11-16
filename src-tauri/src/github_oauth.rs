//! ReleaseHub-owned Device Flow. Tokens are only serialized into SecretStore.
//! No client secret, CLI account, environment token, redirect or remote error text is used.
use crate::{github::{project_account, Account}, proxy::ProxyState, secrets::SecretStore};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, sync::{Arc, Mutex as StdMutex}, time::{SystemTime, UNIX_EPOCH}};
use tokio::sync::{Mutex, Semaphore};

const CLIENT_ID: Option<&str> = option_env!("RELEASEHUB_GITHUB_CLIENT_ID");
const KEY: &str = "github-oauth-session-v1";
const DEVICE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
pub(crate) const DEVICE_URI: &str = "https://github.com/login/device";
const INVALID: &str = "GitHub 登录已失效，请重新连接。";
const STORAGE: &str = "无法将 GitHub 授权保存到系统凭据库，请检查系统安全存储后重试。";
const MAX_BODY: usize = 64 * 1024;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}
fn next_revision(previous: u64) -> u64 { now_ms().max(previous.saturating_add(1)) }
fn valid_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8192 && value.bytes().all(|b| b.is_ascii_graphic())
}
fn parse_scopes(value: &str) -> Vec<String> {
    value.split(|ch: char| ch == ',' || ch.is_ascii_whitespace()).filter(|s| !s.is_empty())
        .filter(|s| s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-')))
        .map(str::to_owned).collect::<BTreeSet<_>>().into_iter().collect()
}

// Deliberately no Debug implementation: even diagnostics must never print tokens.
#[derive(Clone, Serialize, Deserialize)]
struct Credential {
    version: u8,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    refresh_expires_at: Option<u64>,
    scopes: Vec<String>,
    account: Account,
    revision: u64,
    serial: u64,
}
impl Credential {
    fn valid(&self) -> bool {
        self.version == 1 && valid_token(&self.access_token) && self.account.valid()
            && self.refresh_token.as_deref().is_none_or(valid_token)
            && self.scopes.len() <= 100 && self.scopes.iter().all(|s| parse_scopes(s) == [s.clone()])
            && (self.expires_at.is_none_or(|t| t > now_ms()) || self.refresh_usable())
    }
    fn refresh_usable(&self) -> bool {
        self.refresh_token.is_some() && self.refresh_expires_at.is_none_or(|t| t > now_ms())
    }
}
struct State { credential: Option<Credential>, revision: u64, verified: bool, error: Option<String>, dirty: bool }
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub account: Option<Account>, pub scopes: Vec<String>, pub revision: u64,
    pub verified: bool, pub error: Option<String>,
}
pub(crate) struct TokenLease { pub access: String, pub revision: u64, serial: u64 }
pub(crate) struct DeviceGrant { device_code: String, pub user_code: String, pub interval: u64, pub expires_in: u64 }
pub(crate) enum Poll { Pending, SlowDown, Backoff(u64), Complete(PendingCredential) }
pub(crate) struct PendingCredential {
    access_token: String, refresh_token: Option<String>, expires_at: Option<u64>,
    refresh_expires_at: Option<u64>, scopes: Vec<String>,
}

pub(crate) struct OAuth {
    secrets: SecretStore, proxy: Arc<ProxyState>, state: StdMutex<State>, refresh: Mutex<()>, requests: Arc<Semaphore>, enabled: bool,
}
impl OAuth {
    pub fn new(secrets: SecretStore, proxy: Arc<ProxyState>, requests: Arc<Semaphore>, enabled: bool) -> Arc<Self> {
        let loaded = if enabled { secrets.get(KEY) } else { Ok(None) };
        let (credential, error) = match loaded {
            Ok(Some(text)) => match serde_json::from_str::<Credential>(&text) {
                Ok(record) if text.len() <= MAX_BODY && record.valid() => (Some(record), None),
                _ => (None, Some(INVALID.into())),
            },
            Ok(None) => (None, None), Err(error) => (None, Some(error)),
        };
        let revision = credential.as_ref().map_or(0, |c| c.revision);
        Arc::new(Self { secrets, proxy, requests, enabled, state: StdMutex::new(State { credential, revision, verified: false, error, dirty: false }), refresh: Mutex::new(()) })
    }
    pub fn available(&self) -> bool { self.enabled && CLIENT_ID.is_some_and(|id| !id.is_empty()) }
    pub fn local_error(&self, error: String) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
    }
    pub fn snapshot(&self) -> Snapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Snapshot { account: state.credential.as_ref().map(|c| c.account.clone()),
            scopes: state.credential.as_ref().map(|c| c.scopes.clone()).unwrap_or_default(),
            revision: state.revision, verified: state.verified, error: state.error.clone() }
    }
    pub fn requested_scopes(&self, private: bool, lists: bool) -> Vec<String> {
        let mut scopes: BTreeSet<String> = ["read:user", "public_repo", "offline_access"].into_iter().map(str::to_owned).collect();
        scopes.extend(self.snapshot().scopes);
        if private { scopes.insert("repo".into()); }
        if lists { scopes.insert("user".into()); }
        scopes.into_iter().collect()
    }
    pub fn clear(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // Remove persistent authority first. On failure keep a visible error and
        // discard in-memory authority; never report a successful disconnect.
        let result = if state.credential.is_some() || state.error.is_some() { self.secrets.set(KEY, None) } else { Ok(()) };
        state.credential = None;
        state.dirty = false;
        state.revision = next_revision(state.revision);
        state.verified = false;
        state.error = result.as_ref().err().cloned();
        result
    }
    pub fn invalidate(&self, lease: &TokenLease) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.credential.as_ref().is_some_and(|c| c.revision == lease.revision && c.serial == lease.serial) {
            let error = self.secrets.set(KEY, None).err();
            state.credential = None;
            state.dirty = false;
            state.revision = next_revision(state.revision);
            state.verified = false;
            state.error = Some(error.unwrap_or_else(|| INVALID.into()));
        }
    }
    pub async fn token(&self) -> Result<TokenLease, String> {
        // The lock covers refresh HTTP + keyring save. Rotating refresh tokens
        // are never consumed concurrently, including by mutation readbacks.
        let _rotation = self.refresh.lock().await;
        let old = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let old = state.credential.clone().ok_or(INVALID)?;
            if state.dirty { self.save(&old)?; state.dirty = false; state.error = None; }
            old
        };
        if old.expires_at.is_none_or(|t| t > now_ms().saturating_add(60_000)) {
            return Ok(TokenLease { access: old.access_token, revision: old.revision, serial: old.serial });
        }
        if !old.refresh_usable() {
            self.invalidate(&TokenLease { access: String::new(), revision: old.revision, serial: old.serial });
            return Err(INVALID.into());
        }
        let client_id = CLIENT_ID.ok_or("此版本未配置 GitHub 授权，请更新应用。")?;
        let body = oauth_post(&self.proxy, &self.requests, TOKEN_URL, &[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", old.refresh_token.as_deref().unwrap_or(""))]).await?;
        if let Some(error) = body["error"].as_str() {
            if matches!(error, "bad_refresh_token" | "expired_token" | "invalid_grant" | "incorrect_client_credentials") {
                self.invalidate(&TokenLease { access: String::new(), revision: old.revision, serial: old.serial });
            }
            return Err(oauth_error(error));
        }
        let pending = parse_tokens(&body, &old.scopes)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.credential.as_ref().is_some_and(|c| c.revision == old.revision && c.serial == old.serial) { return Err(INVALID.into()); }
        let mut refreshed = Credential { access_token: pending.access_token, refresh_token: pending.refresh_token,
            expires_at: pending.expires_at, refresh_expires_at: pending.refresh_expires_at,
            scopes: pending.scopes, serial: old.serial.saturating_add(1), ..old.clone() };
        if refreshed.scopes != old.scopes { refreshed.revision = next_revision(state.revision); state.revision = refreshed.revision; }
        // Keep the rotated credentials in memory even if OS storage temporarily
        // fails: retrying the old refresh token would destroy recoverability.
        let saved = self.save(&refreshed);
        let lease = TokenLease { access: refreshed.access_token.clone(), revision: refreshed.revision, serial: refreshed.serial };
        state.credential = Some(refreshed);
        state.error = saved.as_ref().err().cloned();
        state.dirty = saved.is_err();
        saved?;
        Ok(lease)
    }
    fn save(&self, credential: &Credential) -> Result<(), String> {
        let text = serde_json::to_string(credential).map_err(|_| STORAGE)?;
        self.secrets.set(KEY, Some(&text)).map_err(|_| STORAGE.to_owned())
    }
    pub async fn verify(&self) -> Result<Account, String> {
        let revision = self.snapshot().revision;
        let result = self.verify_inner().await;
        if let Err(error) = &result {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.revision == revision { state.verified = false; state.error = Some(error.clone()); }
        }
        result
    }
    async fn verify_inner(&self) -> Result<Account, String> {
        let lease = self.token().await?;
        let _permit = self.requests.acquire().await.map_err(|_| "GitHub 请求队列已关闭。")?;
        let response = client(&self.proxy)?.get("https://api.github.com/user")
            .header(header::ACCEPT, "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(&lease.access).send().await.map_err(|_| "暂时无法验证 GitHub 账号，请检查网络后重试。")?;
        if response.status().as_u16() == 401 { self.invalidate(&lease); return Err(INVALID.into()); }
        let account = project_account(&response_json(response).await?)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let credential = state.credential.as_mut().filter(|c| c.revision == lease.revision).ok_or(INVALID)?;
        if credential.account.id != account.id {
            drop(state);
            self.invalidate(&lease);
            return Err("GitHub 账号已变更，请重新连接，再重新确认操作。".into());
        }
        credential.account = account.clone();
        // Identity refresh is not a session change; cache keys remain stable.
        let saved = self.save(credential);
        state.verified = true;
        state.dirty = saved.is_err();
        state.error = saved.err();
        Ok(account)
    }
    pub async fn begin(&self, scopes: &[String]) -> Result<DeviceGrant, String> {
        if !self.available() { return Err("此版本未配置 GitHub 授权，请更新应用。".into()); }
        let scope = scopes.join(" ");
        let value = oauth_post(&self.proxy, &self.requests, DEVICE_URL, &[("client_id", CLIENT_ID.unwrap_or("")), ("scope", &scope)]).await?;
        if let Some(error) = value["error"].as_str() { return Err(oauth_error(error)); }
        parse_device(&value)
    }
    pub async fn poll(&self, grant: &DeviceGrant, scopes: &[String]) -> Result<Poll, String> {
        let _permit = self.requests.acquire().await.map_err(|_| "GitHub 请求队列已关闭。")?;
        let response = client(&self.proxy)?.post(TOKEN_URL).header(header::ACCEPT, "application/json")
            .form(&[("client_id", CLIENT_ID.unwrap_or("")), ("device_code", &grant.device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")])
            .send().await;
        let response = match response {
            Ok(response) => response,
            // A transient transport failure does not consume the Device Flow;
            // back off until its original deadline instead of requesting a new code.
            Err(_) => return Ok(Poll::SlowDown),
        };
        if response.status().as_u16() == 429 || response.status().is_server_error() {
            let delay = response.headers().get(header::RETRY_AFTER).and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok()).unwrap_or(10).clamp(5, 1800);
            return Ok(Poll::Backoff(delay));
        }
        let value = response_json(response).await?;
        match value["error"].as_str() {
            Some("authorization_pending") => Ok(Poll::Pending), Some("slow_down") => Ok(Poll::SlowDown),
            Some(error) => Err(oauth_error(error)), None => Ok(Poll::Complete(parse_tokens(&value, scopes)?)),
        }
    }
    pub async fn finish(&self, pending: PendingCredential, expected_revision: u64) -> Result<(), String> {
        let _permit = self.requests.acquire().await.map_err(|_| "GitHub 请求队列已关闭。")?;
        let response = client(&self.proxy)?.get("https://api.github.com/user")
            .header(header::ACCEPT, "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(&pending.access_token).send().await.map_err(|_| "暂时无法验证 GitHub 账号，请重新授权。")?;
        let account = project_account(&response_json(response).await?)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.revision != expected_revision { return Err("账号状态已变化，请重新发起授权。".into()); }
        if state.credential.as_ref().is_some_and(|old| old.account.id != account.id) {
            return Err("本次授权账号与已连接账号不同，原账号保持连接；切换账号请先断开。".into());
        }
        if state.credential.as_ref().is_some_and(|old| !scopes_cover(&pending.scopes, &old.scopes)) {
            return Err("本次授权未保留已有权限，原有连接保持不变。".into());
        }
        let credential = Credential { version: 1, access_token: pending.access_token, refresh_token: pending.refresh_token,
            expires_at: pending.expires_at, refresh_expires_at: pending.refresh_expires_at, scopes: pending.scopes,
            account, revision: next_revision(state.revision), serial: 0 };
        self.save(&credential)?; // Never replace valid credentials until new authorization is saved.
        state.revision = credential.revision;
        state.credential = Some(credential);
        state.dirty = false;
        state.verified = true;
        state.error = None;
        Ok(())
    }
}

pub(crate) fn client(proxy: &ProxyState) -> Result<Client, String> {
    // ProxyState owns the pool and invalidates it when proxy credentials or
    // settings change. Its GitHub policy is HTTPS-only and never follows redirects.
    proxy.cached_github_client()
}
async fn oauth_post(proxy: &ProxyState, requests: &Semaphore, url: &'static str, form: &[(&str, &str)]) -> Result<Value, String> {
    // Both callers pass a compiled-in GitHub URL, never user input.
    let _permit = requests.acquire().await.map_err(|_| "GitHub 请求队列已关闭。")?;
    let response = client(proxy)?.post(url).header(header::ACCEPT, "application/json").form(form)
        .send().await.map_err(|_| "暂时无法连接 GitHub 授权服务，请检查网络后重试。")?;
    response_json(response).await
}
async fn response_json(mut response: reqwest::Response) -> Result<Value, String> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status { 401 => INVALID, 403 | 429 => "GitHub 授权请求受到限制，请稍后再试。",
            300..=399 => "GitHub 授权服务返回了跳转，已停止请求。", _ => "GitHub 授权服务暂时不可用，请稍后再试。" }.into());
    }
    if response.content_length().is_some_and(|n| n > MAX_BODY as u64) { return Err("GitHub 授权响应过大。".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取 GitHub 授权响应失败。")? {
        if bytes.len() + chunk.len() > MAX_BODY { return Err("GitHub 授权响应过大。".into()); }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "GitHub 授权响应格式不正确。".into())
}
fn parse_device(value: &Value) -> Result<DeviceGrant, String> {
    let code = value["user_code"].as_str().filter(|code| {
        let bytes = code.as_bytes(); bytes.len() == 9 && bytes[4] == b'-' && bytes.iter().enumerate().all(|(i,b)| i == 4 || b.is_ascii_uppercase() || b.is_ascii_digit())
    }).ok_or("GitHub 授权短码格式不正确。")?;
    if value["verification_uri"].as_str() != Some(DEVICE_URI) { return Err("GitHub 授权页面地址不正确。".into()); }
    Ok(DeviceGrant {
        device_code: value["device_code"].as_str().filter(|v| valid_token(v)).ok_or("GitHub 授权响应不完整。")?.to_owned(),
        user_code: code.to_owned(), interval: value["interval"].as_u64().filter(|n| (1..=60).contains(n)).unwrap_or(5).max(5),
        expires_in: value["expires_in"].as_u64().filter(|n| (1..=1800).contains(n)).ok_or("GitHub 授权有效期不正确。")?,
    })
}
fn parse_tokens(value: &Value, requested: &[String]) -> Result<PendingCredential, String> {
    if !value["token_type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("bearer")) { return Err("GitHub 授权类型不正确。".into()); }
    let access_token = value["access_token"].as_str().filter(|s| valid_token(s)).ok_or("GitHub 授权响应不完整。")?.to_owned();
    let refresh_token = value.get("refresh_token").map(|v| v.as_str().filter(|s| valid_token(s)).map(str::to_owned).ok_or("GitHub 刷新授权格式不正确。")).transpose()?;
    let expiry = |key: &str| -> Result<Option<u64>, String> {
        value.get(key).map(|v| v.as_u64().filter(|n| *n > 0 && *n <= 366 * 24 * 60 * 60)
            .map(|n| now_ms().saturating_add(n * 1000)).ok_or_else(|| "GitHub 授权有效期不正确。".into())).transpose()
    };
    let scopes = match value["scope"].as_str() { Some(scope) => parse_scopes(scope), None => requested.to_vec() };
    Ok(PendingCredential { access_token, refresh_token, expires_at: expiry("expires_in")?, refresh_expires_at: expiry("refresh_token_expires_in")?, scopes })
}
fn scopes_cover(granted: &[String], requested: &[String]) -> bool {
    requested.iter().all(|scope| granted.contains(scope) || scope == "offline_access"
        || (scope == "read:user" && granted.iter().any(|s| s == "user"))
        || (scope == "public_repo" && granted.iter().any(|s| s == "repo")))
}
fn oauth_error(error: &str) -> String {
    match error {
        "access_denied" => "已拒绝本次 GitHub 授权，原有连接保持不变。",
        "expired_token" => "GitHub 授权短码已过期，请重新获取。",
        "bad_refresh_token" | "invalid_grant" => INVALID,
        "unsupported_grant_type" | "incorrect_client_credentials" | "device_flow_disabled" => "此版本的 GitHub 授权配置不可用，请更新应用。",
        _ => "GitHub 授权未完成，请稍后重新获取短码。",
    }.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn device_response_is_pinned_bounded_and_not_a_remote_instruction() {
        let mut value = json!({"device_code":"fixture-only", "user_code":"ABCD-EFGH", "verification_uri":DEVICE_URI, "expires_in":900, "interval":1});
        let device = parse_device(&value).unwrap();
        assert_eq!(device.user_code, "ABCD-EFGH"); assert_eq!(device.interval, 5);
        value["verification_uri"] = json!("https://untrusted.example/login"); assert!(parse_device(&value).is_err());
        value["verification_uri"] = json!(DEVICE_URI); value["user_code"] = json!("ABCD-EFGH extra"); assert!(parse_device(&value).is_err());
    }
    #[test]
    fn token_response_rejects_header_injection_and_bounds_expiry() {
        let mut value = json!({"access_token":"fixture-only", "token_type":"bearer", "scope":"repo,read:user repo", "expires_in":3600});
        let token = parse_tokens(&value, &[]).unwrap(); assert_eq!(token.scopes, ["read:user", "repo"]);
        assert!(token.expires_at.unwrap() > now_ms());
        value["access_token"] = json!("invalid\r\nheader"); assert!(parse_tokens(&value, &[]).is_err());
        value["access_token"] = json!("fixture-only"); value["expires_in"] = json!(0); assert!(parse_tokens(&value, &[]).is_err());
        assert!(!oauth_error("arbitrary remote secret").contains("arbitrary remote secret"));
    }
    #[test]
    fn persisted_credentials_require_numeric_identity_and_usable_lifetime() {
        let mut credential = Credential { version:1, access_token:"fixture-only".into(), refresh_token:None,
            expires_at:None, refresh_expires_at:None, scopes:vec!["read:user".into()],
            account:project_account(&json!({"id":42,"login":"octocat"})).unwrap(), revision:7, serial:0 };
        assert!(credential.valid());
        credential.account.id = 0; assert!(!credential.valid()); credential.account.id = 42;
        credential.expires_at = Some(now_ms().saturating_sub(1)); assert!(!credential.valid());
        credential.refresh_token = Some("refresh-fixture-only".into()); credential.refresh_expires_at = Some(now_ms()+60000);
        assert!(credential.valid());
        credential.refresh_expires_at = Some(now_ms().saturating_sub(1)); assert!(!credential.valid());
        credential.expires_at = None; credential.scopes = vec!["repo user".into()]; assert!(!credential.valid());
    }
    #[test]
    fn permission_expansion_preserves_existing_effective_scopes() {
        let scopes = |values: &[&str]| values.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(scopes_cover(&scopes(&["repo", "user"]), &scopes(&["public_repo", "read:user", "offline_access"])));
        assert!(!scopes_cover(&scopes(&["public_repo", "read:user"]), &scopes(&["repo", "user"])));
        assert!(!scopes_cover(&scopes(&["repo"]), &scopes(&["read:user"])));
    }
    #[test]
    fn cached_identity_snapshot_is_immediate_and_contains_no_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = SecretStore::for_data_directory(temp.path()).unwrap();
        let proxy = Arc::new(ProxyState::new(crate::proxy::ProxySettings {mode:"direct".into(), ..Default::default()}, None).unwrap());
        let oauth = OAuth::new(secrets, proxy, Arc::new(Semaphore::new(4)), false);
        let credential = Credential { version:1, access_token:"fixture-only".into(), refresh_token:None,
            expires_at:None, refresh_expires_at:None, scopes:vec!["public_repo".into()],
            account:project_account(&json!({"id":42,"login":"octocat"})).unwrap(), revision:7, serial:0 };
        {
            let mut state = oauth.state.lock().unwrap(); state.credential = Some(credential); state.revision = 7;
            state.error = Some("暂时网络失败".into());
        }
        let snapshot = oauth.snapshot();
        assert_eq!(snapshot.account.unwrap().id, 42); assert!(!snapshot.verified);
        assert_eq!(snapshot.revision, 7); assert!(snapshot.error.is_some());
        assert_eq!(oauth.requested_scopes(true, true), ["offline_access", "public_repo", "read:user", "repo", "user"]);
        // This test never saves a fixture credential or starts real authorization.
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }
    #[test]
    fn revisions_are_monotonic_and_scopes_normalized() {
        let current = now_ms() + 1000; assert_eq!(next_revision(current), current + 1);
        assert_eq!(parse_scopes("repo, user read:user,repo"), ["read:user", "repo", "user"]);
    }
}
