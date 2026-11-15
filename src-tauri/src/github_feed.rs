//! GitHub's actual, separately signed-in website feed. No gh token, browser-cookie
//! import, RSS, Events reconstruction, portable session data, or remote dispatch.
use crate::proxy::{ProxySettings, ProxyState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tauri::{webview::{NewWindowResponse, PageLoadEvent}, AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::sync::Mutex as AsyncMutex;
use url::Url;
use uuid::Uuid;

const WINDOW_LABEL: &str = "github-feed";
const SOURCE: &str = "https://github.com/dashboard-feed";
const MAX_ITEMS: usize = 160;
const MAX_PAYLOAD: usize = 2 * 1024 * 1024;
const EXTRACTOR: &str = include_str!("../../src/lib/feed/dom-extractor.js");
const RECEIVER: &str = include_str!("../../src/lib/feed/webview-receiver.js");

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GithubFeedItem {
    pub id: String,
    pub actor: String,
    pub avatar_url: Option<String>,
    pub summary: String,
    pub body: String,
    pub url: String,
    pub repository: Option<String>,
    pub occurred_at: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubFeedPage {
    pub connected: bool,
    pub loading: bool,
    pub account: Option<String>,
    pub items: Vec<GithubFeedItem>,
    pub has_more: bool,
    pub last_updated: Option<String>,
    pub error: Option<String>,
    pub source_url: String,
    pub request_id: String,
    pub session_revision: u64,
}
impl Default for GithubFeedPage {
    fn default() -> Self {
        Self { connected: false, loading: false, account: None, items: Vec::new(), has_more: false,
            last_updated: None, error: None, source_url: SOURCE.into(), request_id: Uuid::new_v4().to_string(), session_revision: 0 }
    }
}

/// Only plain bounded feed fields cross the remote capability. Never accept an
/// action, a path, arbitrary HTML, a script, or a URL to fetch from this receiver.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedReceivePayload {
    request_id: String,
    window_token: String,
    document_id: String,
    sequence: u64,
    status: String,
    connected: bool,
    account: Option<String>,
    items: Vec<GithubFeedItem>,
    has_more: bool,
    source_url: String,
    error: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileRecord { id: Uuid, proxy_fingerprint: String }
struct FeedState {
    page: GithubFeedPage,
    window_token: Option<String>,
    proxy_fingerprint: Option<String>,
    document_id: Option<String>,
    sequence: u64,
    started: Option<Instant>,
    append: bool,
    hide_when_ready: bool,
}
impl Default for FeedState {
    fn default() -> Self {
        Self { page: GithubFeedPage::default(), window_token: None, proxy_fingerprint: None,
            document_id: None, sequence: 0, started: None, append: false, hide_when_ready: false }
    }
}

pub struct GitHubFeedService {
    machine_local_root: PathBuf,
    proxy: Arc<ProxyState>,
    state: Mutex<FeedState>,
    operation: AsyncMutex<()>,
}
impl GitHubFeedService {
    pub fn new(machine_local_root: PathBuf, proxy: Arc<ProxyState>) -> Arc<Self> {
        Arc::new(Self { machine_local_root, proxy, state: Mutex::new(FeedState::default()), operation: AsyncMutex::new(()) })
    }

    pub async fn handle(self: &Arc<Self>, app: &AppHandle, action: &str, payload: Value) -> Result<Value, String> {
        if !payload.is_object() || payload.as_object().is_some_and(|value| !value.is_empty()) {
            return Err("动态操作不接受额外参数。".into());
        }
        if action == "feed_status" { return self.snapshot(); }
        let _operation = self.operation.lock().await;
        let result = match action {
            "feed_connect" => self.open(app, true).await,
            "feed_refresh" => self.open(app, false).await,
            "feed_more" => self.more(app).await,
            "feed_disconnect" => self.disconnect(app).await,
            _ => return Err("不支持的首页动态操作。".into()),
        };
        if let Err(error) = result {
            let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
            state.page.loading = false;
            state.page.error = Some(error);
            state.started = None;
        }
        self.snapshot()
    }

    /// The host calls this after saving proxy credentials/settings. WebView2's
    /// proxy is a startup setting; merely changing the HTTP client is not enough.
    pub async fn restart_for_proxy_change(&self, app: &AppHandle) -> Result<(), String> {
        let _operation = self.operation.lock().await;
        self.reset_session(Some("代理已更新。重新连接网页后会使用新代理。".into()))?;
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            window.destroy().map_err(|_| "请关闭 GitHub 网页窗口，再重新连接以应用代理。")?;
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Value, String> {
        let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
        if state.page.loading && state.started.is_some_and(|at| at.elapsed() > Duration::from_secs(35)) {
            state.page.loading = false;
            state.page.error = Some("官网读取超时。打开官网窗口检查登录或网络，再点刷新。".into());
            state.started = None;
        }
        serde_json::to_value(&state.page).map_err(|_| "无法读取动态状态。".into())
    }

    fn begin(&self, append: bool, clear: bool) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
        state.page.request_id = Uuid::new_v4().to_string();
        state.page.loading = true;
        state.page.error = None;
        if clear { state.page.items.clear(); state.page.has_more = false; state.page.last_updated = None; }
        state.append = append;
        state.sequence = 0;
        state.document_id = None;
        state.started = Some(Instant::now());
        Ok(state.page.request_id.clone())
    }

    fn reset_session(&self, error: Option<String>) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
        let revision = state.page.session_revision + 1;
        *state = FeedState::default();
        state.page.session_revision = revision;
        state.page.error = error;
        Ok(())
    }

    fn script(&self, token: &str) -> Option<String> {
        let state = self.state.lock().ok()?;
        if state.window_token.as_deref() != Some(token) { return None; }
        let config = json!({"requestId": state.page.request_id, "windowToken": token});
        Some(format!("(()=>{{'use strict';\n{EXTRACTOR}\n{RECEIVER}\nstartReleaseHubFeed({config});\n}})();"))
    }

    async fn open(self: &Arc<Self>, app: &AppHandle, visible: bool) -> Result<(), String> {
        let settings = self.proxy.settings()?;
        let fingerprint = proxy_fingerprint(&settings);
        let current = self.state.lock().map_err(|_| "动态状态暂时不可用。")?.proxy_fingerprint.clone();
        let stale_window = app.get_webview_window(WINDOW_LABEL).is_some() && self.state.lock().map_err(|_| "动态状态暂时不可用。")?.window_token.is_none();
        if stale_window || current.as_deref().is_some_and(|old| old != fingerprint) {
            self.reset_session(None)?;
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) { window.destroy().map_err(|_| "无法重启 GitHub 网页窗口。")?; }
        }
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            self.begin(false, !visible)?;
            if visible {
                // A deliberate reopen stays visible for account changes/site inspection.
                self.state.lock().map_err(|_| "动态状态暂时不可用。")?.hide_when_ready = false;
                window.show().map_err(|_| "无法显示 GitHub 网页窗口。")?;
                window.set_focus().map_err(|_| "无法激活 GitHub 网页窗口。")?;
            }
            let url = window.url().map_err(|_| "无法读取 GitHub 网页地址。")?;
            if !visible || !is_feed_url(&url) {
                window.navigate(source_url()).map_err(|_| "无法刷新 GitHub 官网动态。")?;
            } else {
                let token = self.state.lock().map_err(|_| "动态状态暂时不可用。")?.window_token.clone();
                if let Some(script) = token.as_deref().and_then(|token| self.script(token)) { window.eval(script).map_err(|_| "无法读取 GitHub 官网动态。")?; }
            }
            return Ok(());
        }

        if settings.mode == "manual" && !settings.username.is_empty() {
            return Err("网页登录暂不使用带账号密码的手动代理。请在设置中选择系统代理，或使用本机无须密码的 HTTP / SOCKS5 代理，再连接网页。".into());
        }
        let profile = self.profile(&fingerprint)?;
        // This runs under the operation lock: a delayed cleanup must never race a
        // newly rotated profile pointer and delete a newer login session.
        let cleanup_warning = remove_retired_profiles(&self.profile_root()?).await.err();
        let token = Uuid::new_v4().to_string();
        self.begin(false, true)?;
        {
            let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
            state.window_token = Some(token.clone());
            state.proxy_fingerprint = Some(fingerprint);
            state.hide_when_ready = true;
            state.page.error = cleanup_warning.map(|_| "上次网页登录的旧文件仍被占用；本次连接使用独立会话，关闭追新后可完成清理。".into());
        }
        let script = self.script(&token).ok_or("无法准备 GitHub 网页连接。")?;
        let weak = Arc::downgrade(self);
        let page_token = token.clone();
        let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(source_url()))
            .title("GitHub 官网登录 · 追新")
            .inner_size(1080.0, 760.0).min_inner_size(800.0, 600.0).center()
            .visible(visible).data_directory(profile).initialization_script(script)
            .on_navigation(is_github_url)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, _| false)
            .on_page_load(move |window, event| {
                let Some(service) = weak.upgrade() else { return; };
                if !is_github_url(event.url()) { return; }
                if event.event() == PageLoadEvent::Started {
                    if let Ok(mut state) = service.state.lock() {
                        if state.window_token.as_deref() != Some(&page_token) { return; }
                        state.page.request_id = Uuid::new_v4().to_string();
                        state.document_id = None; state.sequence = 0;
                        state.page.loading = true; state.started = Some(Instant::now());
                        if is_login_url(event.url()) { clear_account(&mut state, None); }
                    }
                } else if let Some(script) = service.script(&page_token) { let _ = window.eval(script); }
            });
        // Do not combine additional_browser_args with proxy_url: Wry 0.57's
        // custom args replace (not append to) the generated proxy arguments.
        builder = match settings.mode.as_str() {
            "direct" => builder.additional_browser_args("--no-proxy-server"),
            "manual" => {
                let mut url = self.proxy.webview_proxy_url()?.ok_or("手动代理地址不存在。")?;
                if url.scheme() == "socks5h" { url.set_scheme("socks5").map_err(|_| "代理格式无效。")?; }
                if !matches!(url.scheme(), "http" | "socks5") { return Err("网页登录的手动代理只支持 HTTP 或 SOCKS5，请选择系统代理或调整地址。".into()); }
                builder.proxy_url(url)
            }
            _ => builder, // Native Windows settings/PAC, not another tool's cookies.
        };
        let window = builder.build().map_err(|_| "无法创建 GitHub 官网窗口。请检查 WebView2 和代理设置后重试。")?;
        let window_handle = window.clone();
        let weak = Arc::downgrade(self);
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_handle.hide();
                if let Some(service) = weak.upgrade() {
                    if let Ok(mut state) = service.state.lock() {
                        if state.window_token.as_deref() != Some(&token) { return; }
                        state.hide_when_ready = false;
                        if !state.page.connected {
                            state.page.loading = false; state.started = None;
                            state.page.error = Some("网页登录尚未完成。点“连接官网”可继续。".into());
                        }
                    }
                }
            }
        });
        Ok(())
    }

    async fn more(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        let settings = self.proxy.settings()?;
        let fingerprint = proxy_fingerprint(&settings);
        {
            let state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
            if state.proxy_fingerprint.as_deref() != Some(&fingerprint) { return Err("代理已变化，请先刷新动态以重启网页连接。".into()); }
            if !state.page.connected { return Err("请先在官网窗口登录 GitHub。".into()); }
            if state.page.loading { return Ok(()); }
            if !state.page.has_more || state.page.items.len() >= MAX_ITEMS { return Ok(()); }
        }
        let window = app.get_webview_window(WINDOW_LABEL).ok_or("官网窗口已关闭，请先刷新动态。")?;
        let request = self.begin(true, false)?;
        window.eval(format!("window.__releasehubFeedReader?.request({},'more');", json!(request))).map_err(|_| "无法加载更多官网动态。".into())
    }

    async fn disconnect(&self, app: &AppHandle) -> Result<(), String> {
        self.reset_session(None)?;
        // Rotate the pointer BEFORE asynchronous WebView2 cleanup. Even if its
        // process briefly locks old files, a new connection cannot revive them.
        self.rotate_profile("")?;
        let mut clear_failed = false;
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            clear_failed = window.clear_all_browsing_data().is_err();
            window.destroy().map_err(|_| "已退出动态，请关闭 GitHub 网页窗口以完成本机会话清理。")?;
        }
        let root = self.profile_root()?;
        let removed = remove_retired_profiles(&root).await;
        if removed.is_err() || clear_failed {
            return Err("已退出并停用原会话。网页进程仍占用部分本机文件；关闭追新后再次打开即可继续清理。".into());
        }
        Ok(())
    }

    fn profile_root(&self) -> Result<PathBuf, String> {
        if !self.machine_local_root.is_absolute() { return Err("网页登录目录必须是本机绝对路径。".into()); }
        let root = self.machine_local_root.join("github-feed-webview");
        if fs::symlink_metadata(&root).is_ok_and(|metadata| metadata.file_type().is_symlink()) { return Err("网页登录目录不能是链接。".into()); }
        fs::create_dir_all(&root).map_err(|_| "无法建立本机网页登录目录。")?;
        root.canonicalize().map_err(|_| "无法定位本机网页登录目录。".into())
    }

    fn rotate_profile(&self, fingerprint: &str) -> Result<ProfileRecord, String> {
        let root = self.profile_root()?;
        let next = ProfileRecord { id: Uuid::new_v4(), proxy_fingerprint: fingerprint.into() };
        fs::write(root.join("current.json"), serde_json::to_vec(&next).map_err(|_| "无法保存网页登录配置。")?).map_err(|_| "无法保存本机网页登录配置。")?;
        Ok(next)
    }

    fn profile(&self, fingerprint: &str) -> Result<PathBuf, String> {
        let root = self.profile_root()?;
        let record = read_profile(&root).filter(|record| record.proxy_fingerprint == fingerprint);
        let record = match record { Some(record) => record, None => self.rotate_profile(fingerprint)? };
        let path = root.join(format!("profile-{}", record.id));
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) { return Err("网页登录目录不能是链接。".into()); }
        fs::create_dir_all(&path).map_err(|_| "无法建立网页登录数据目录。")?;
        // Current data is never exported or exposed to the page. Cleanup is
        // awaited by open() under the same operation lock as profile rotation.
        Ok(path)
    }

    fn receive(&self, window: &WebviewWindow, payload: FeedReceivePayload) -> Result<(), String> {
        if window.label() != WINDOW_LABEL { return Err("动态接收仅用于独立官网窗口。".into()); }
        let actual_url = window.url().map_err(|_| "无法验证网页来源。")?;
        if !is_github_url(&actual_url) { return Err("动态接收仅允许 GitHub 官方网页。".into()); }
        if payload.items.len() > MAX_ITEMS || serde_json::to_vec(&payload).map_err(|_| "动态格式无效。")?.len() > MAX_PAYLOAD {
            return Err("动态内容超过单次读取上限。".into());
        }
        if !["ready", "loading", "login", "other", "error"].contains(&payload.status.as_str()) || Uuid::parse_str(&payload.document_id).is_err() {
            return Err("动态格式无效。".into());
        }
        let reported_url = Url::parse(&payload.source_url).map_err(|_| "动态来源无效。")?;
        if !is_github_url(&reported_url) || actual_url.path() != reported_url.path() { return Ok(()); }
        let mut state = self.state.lock().map_err(|_| "动态状态暂时不可用。")?;
        if state.window_token.as_deref() != Some(&payload.window_token) || state.page.request_id != payload.request_id { return Ok(()); }
        if state.document_id.as_deref().is_some_and(|id| id != payload.document_id) || payload.sequence <= state.sequence { return Ok(()); }
        state.document_id = Some(payload.document_id); state.sequence = payload.sequence;
        let account = payload.account.filter(|name| valid_login(name));
        if state.page.account != account {
            clear_account(&mut state, account.clone());
        }
        state.page.connected = payload.connected && account.is_some();
        state.page.account = account;
        if is_feed_url(&reported_url) { state.page.source_url = format!("https://github.com{}", reported_url.path()); }
        match payload.status.as_str() {
            "login" => {
                clear_account(&mut state, None);
                state.page.loading = false; state.started = None;
                state.page.error = payload.error.map(|error| plain_text(&error, 400));
                // An expired session needs the real page, not an invisible login wait.
                drop(state); let _ = window.show(); return Ok(());
            }
            "ready" if state.page.connected && is_feed_url(&reported_url) => {
                let items: Vec<_> = payload.items.into_iter().filter_map(sanitize_item).collect();
                if state.append { append_unique(&mut state.page.items, items); } else { state.page.items = unique_items(items); }
                state.page.has_more = payload.has_more && state.page.items.len() < MAX_ITEMS;
                state.page.loading = false;
                state.page.error = None;
                state.page.last_updated = Some(Utc::now().to_rfc3339()); state.started = None;
                let hide = state.hide_when_ready; state.hide_when_ready = false;
                drop(state); if hide { let _ = window.hide(); } return Ok(());
            }
            "error" => {
                state.page.loading = false; state.started = None;
                state.page.error = Some(payload.error.map(|error| plain_text(&error, 400)).filter(|error| !error.is_empty()).unwrap_or_else(|| "官网动态暂时没有读到，请打开官网窗口检查后重试。".into()));
            }
            "other" => {
                state.page.loading = false; state.started = None;
                state.page.error = Some("官网窗口当前不在动态页。点击刷新可返回 Dashboard feed。".into());
            }
            _ => { state.page.loading = true; if state.started.is_none() { state.started = Some(Instant::now()); } }
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn feed_receive(window: WebviewWindow, service: State<'_, Arc<GitHubFeedService>>, payload: FeedReceivePayload) -> Result<(), String> {
    service.receive(&window, payload)
}

fn clear_account(state: &mut FeedState, account: Option<String>) {
    if state.page.account != account { state.page.session_revision += 1; }
    state.page.account = account;
    state.page.connected = state.page.account.is_some();
    state.page.items.clear(); state.page.has_more = false; state.page.last_updated = None;
    state.append = false;
}
fn source_url() -> Url { Url::parse(SOURCE).expect("constant GitHub URL") }
fn is_github_url(url: &Url) -> bool { url.scheme() == "https" && url.host_str() == Some("github.com") && url.port().is_none() && url.username().is_empty() && url.password().is_none() }
fn is_feed_url(url: &Url) -> bool { is_github_url(url) && matches!(url.path(), "/" | "/dashboard" | "/dashboard-feed") }
fn is_login_url(url: &Url) -> bool { matches!(url.path().split('/').nth(1), Some("login" | "logout" | "session" | "sessions")) }
fn valid_login(value: &str) -> bool { !value.is_empty() && value.len() <= 39 && value.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric()) && value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') }
fn plain_text(value: &str, maximum: usize) -> String { value.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(maximum).collect::<String>().trim().to_owned() }
fn safe_url(value: &str, avatar: bool) -> Option<String> {
    if value.len() > 2048 { return None; }
    let url = Url::parse(value).ok()?;
    let host = if avatar { "avatars.githubusercontent.com" } else { "github.com" };
    (url.scheme() == "https" && url.host_str() == Some(host) && url.port().is_none() && url.username().is_empty() && url.password().is_none()).then(|| url.to_string())
}
fn valid_repository(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    parts.len() == 2 && valid_login(parts[0]) && !parts[1].is_empty() && parts[1].len() <= 100 && !matches!(parts[1], "." | "..") && parts[1].bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}
fn sanitize_item(item: GithubFeedItem) -> Option<GithubFeedItem> {
    let summary = plain_text(&item.summary, 500);
    let id = plain_text(&item.id, 160);
    if summary.is_empty() || id.is_empty() { return None; }
    Some(GithubFeedItem { id, actor: plain_text(&item.actor, 100), avatar_url: item.avatar_url.and_then(|url| safe_url(&url, true)), summary,
        body: plain_text(&item.body, 1800), url: safe_url(&item.url, false)?, repository: item.repository.filter(|repo| valid_repository(repo)),
        occurred_at: item.occurred_at.and_then(|value| DateTime::parse_from_rfc3339(&value).ok().map(|date| date.to_rfc3339())) })
}
fn unique_items(items: Vec<GithubFeedItem>) -> Vec<GithubFeedItem> { let mut output = Vec::new(); append_unique(&mut output, items); output }
fn append_unique(output: &mut Vec<GithubFeedItem>, items: Vec<GithubFeedItem>) {
    let mut seen: HashSet<String> = output.iter().map(|item| item.id.clone()).collect();
    for item in items { if output.len() >= MAX_ITEMS { break; } if seen.insert(item.id.clone()) { output.push(item); } }
}
fn proxy_fingerprint(settings: &ProxySettings) -> String { format!("{:x}", Sha256::digest(format!("{}\n{}\n{}", settings.mode, settings.url, settings.username))) }
fn read_profile(root: &Path) -> Option<ProfileRecord> {
    let path = root.join("current.json");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 1024 { return None; }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
async fn remove_retired_profiles(root: &Path) -> Result<(), String> {
    let active = read_profile(root).ok_or("无法读取本机会话目录记录。")?.id;
    for attempt in 0..5 {
        let mut locked = false;
        for entry in fs::read_dir(root).map_err(|_| "无法清理本机会话目录。")? {
            let entry = entry.map_err(|_| "无法读取本机会话目录。")?;
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| name.strip_prefix("profile-")).and_then(|id| Uuid::parse_str(id).ok()) else { continue; };
            if id == active { continue; }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| "无法检查本机会话目录。")?;
            // Never follow a directory junction/symlink or remove outside our root.
            if !metadata.is_dir() || metadata.file_type().is_symlink() || path.parent() != Some(root) { continue; }
            let canonical = path.canonicalize().map_err(|_| "无法确认本机会话目录。")?;
            if canonical.parent() != Some(root) { continue; }
            if fs::remove_dir_all(&canonical).is_err() { locked = true; }
        }
        if !locked { return Ok(()); }
        if attempt < 4 { tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await; }
    }
    Err("网页进程仍占用旧会话目录。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> GithubFeedItem { GithubFeedItem { id: id.into(), actor: "sample".into(), summary: "sample release".into(), url: "https://github.com/example/project/releases/tag/v1".into(), ..Default::default() } }
    #[test]
    fn receiver_urls_are_exact_https_hosts() {
        for value in ["https://github.com.evil.test/", "http://github.com/", "https://user@github.com/", "https://github.com:444/", "javascript:alert(1)"] { assert!(safe_url(value, false).is_none()); }
        assert!(safe_url("https://github.com/example/project", false).is_some());
        assert!(safe_url("https://avatars.githubusercontent.com/u/1", true).is_some());
    }
    #[test]
    fn sanitizes_remote_fields_and_drops_bad_links() {
        let mut raw = item("one"); raw.avatar_url = Some("https://evil.test/avatar".into()); raw.body = "<script>not executed</script>\u{202e}".repeat(300); raw.repository = Some("../bad".into());
        let clean = sanitize_item(raw).unwrap(); assert_eq!(clean.avatar_url, None); assert_eq!(clean.repository, None); assert!(clean.body.chars().count() <= 1800); assert!(!clean.body.contains('\u{202e}'));
        let mut bad = item("two"); bad.url = "javascript:alert(1)".into(); assert!(sanitize_item(bad).is_none());
    }
    #[test]
    fn pagination_preserves_page_order_and_limit() {
        let mut output = vec![item("first"), item("second")]; append_unique(&mut output, vec![item("second"), item("third")]);
        assert_eq!(output.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), vec!["first", "second", "third"]);
        append_unique(&mut output, (0..300).map(|n| item(&n.to_string())).collect()); assert_eq!(output.len(), MAX_ITEMS);
    }
    #[test]
    fn account_change_clears_previous_private_content() {
        let mut state = FeedState::default(); state.page.account = Some("old-user".into()); state.page.items = vec![item("old")]; state.append = true;
        clear_account(&mut state, Some("new-user".into())); assert!(state.page.items.is_empty()); assert!(!state.append); assert_eq!(state.page.session_revision, 1);
    }
    #[tokio::test]
    async fn cleanup_only_removes_retired_uuid_profiles() {
        let temp = tempfile::tempdir().unwrap(); let root = temp.path().canonicalize().unwrap(); let active = Uuid::new_v4(); let old = Uuid::new_v4();
        fs::write(root.join("current.json"), serde_json::to_vec(&ProfileRecord { id: active, proxy_fingerprint: "test".into() }).unwrap()).unwrap();
        for name in [format!("profile-{active}"), format!("profile-{old}"), "unrelated".into()] { fs::create_dir(root.join(name)).unwrap(); }
        remove_retired_profiles(&root).await.unwrap(); assert!(root.join(format!("profile-{active}")).exists()); assert!(!root.join(format!("profile-{old}")).exists()); assert!(root.join("unrelated").exists());
    }
}
