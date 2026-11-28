//! The official FLiNG site runs in a separate, script-disabled child WebView. The WebView
//! never writes downloads; the main UI must confirm a recognized link and pick a local path.
use super::bounds;
use crate::{core::release_links::{credential_free_https, decode_path_segment}, proxy::ProxyState};
use futures_util::StreamExt;
use reqwest::{header, Client, Response, StatusCode};
use serde_json::{json, Value};
use std::{fs, io::ErrorKind, path::{Component, Path, PathBuf}, sync::Mutex, time::Duration};
use tauri::{webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, PermissionResponse, WebviewBuilder}, AppHandle, Emitter, EventTarget, Manager, WebviewUrl};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

const PREFIX: &str = "trainer-reader-";
const HOME: &str = "https://flingtrainer.com/";
const HOST: &str = "flingtrainer.com";
const MAX_BYTES: u64 = 512 * 1024 * 1024;
// FLiNG's file endpoint rejects requests without a browser agent and a same-site Referer.
const DOWNLOAD_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36";
const BAD_LINK: &str = "这不是可在追新保存的风灵月影官网附件。";
const CLOSED: &str = "官网视图已关闭，下载未保存。";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Archive { year: String, month: String, name: String }

#[derive(Debug, PartialEq, Eq)]
enum Attachment {
    Ticket,
    Redirect(Archive),
    Zip(Archive),
    File(Archive, String),
}

impl Attachment {
    fn file_name(&self) -> Option<&str> {
        match self { Self::Zip(a) => Some(&a.name), Self::File(_, name) => Some(name), _ => None }
    }

    fn follows(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Ticket, Self::Redirect(_)) => true,
            (Self::Redirect(expected), Self::Zip(actual) | Self::File(actual, _)) => expected == actual,
            _ => false,
        }
    }
}

fn official(url: &Url) -> bool { credential_free_https(url, HOST) && url.as_str().len() <= 4096 }

fn plain_name(name: &str) -> bool {
    if name.is_empty() || name.encode_utf16().count() > 230 || name.ends_with([' ', '.']) ||
        name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c) || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") &&
        !["COM", "LPT"].into_iter().any(|prefix| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "0"|"1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"|"¹"|"²"|"³")))
}

fn extension(name: &str, ext: &str) -> bool { name.rsplit_once('.').is_some_and(|(stem, value)| !stem.is_empty() && value.eq_ignore_ascii_case(ext)) }

fn archive(year: &str, month: &str, name: String) -> Option<Archive> {
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) || month.len() != 2 ||
        !month.bytes().all(|b| b.is_ascii_digit()) || !month.parse::<u8>().ok().is_some_and(|n| (1..=12).contains(&n)) ||
        !plain_name(&name) || !extension(&name, "zip") { return None; }
    Some(Archive { year: year.into(), month: month.into(), name })
}

fn attachment(url: &Url) -> Option<Attachment> {
    if !official(url) || url.fragment().is_some() { return None; }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.as_slice() == ["download-trainer.php"] {
        let mut query = url.query_pairs();
        let (field, value) = query.next()?;
        if field != "path" || query.next().is_some() { return None; }
        return match value.split('/').collect::<Vec<_>>().as_slice() {
            ["", "wp-content", "uploads", year, month, name] => Some(Attachment::Redirect(archive(year, month, (*name).into())?)),
            _ => None,
        };
    }
    if url.query().is_some() { return None; }
    match parts.as_slice() {
        ["downloads", ticket] if (16..=100).contains(&ticket.len()) && ticket.trim_end_matches(',').len() >= 16 &&
            ticket.trim_end_matches(',').bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') &&
            ticket.len() - ticket.trim_end_matches(',').len() <= 2 => Some(Attachment::Ticket),
        ["wp-content", "uploads", year, month, name] => Some(Attachment::Zip(archive(year, month, decode_path_segment(name)?)?)),
        ["wp-content", "uploads", "trainer-files", year, month, name, file] => {
            let file = decode_path_segment(file)?;
            if !plain_name(&file) || !(extension(&file, "exe") || extension(&file, "zip")) { return None; }
            Some(Attachment::File(archive(year, month, decode_path_segment(name)?)?, file))
        }
        _ => None,
    }
}

fn parse_url(raw: &str) -> Option<Url> {
    if raw.len() > 4096 || raw.chars().any(|c| c == '\\' || c.is_control()) || raw.contains('\u{fffd}') { return None; }
    Url::parse(raw).ok().filter(official)
}

fn segment(value: &str) -> bool {
    decode_path_segment(value).is_some_and(|word| !matches!(word.as_str(), "." | "..") && word.len() <= 256 &&
        word.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-')))
}

fn page_number(value: &str) -> bool { !value.is_empty() && value.len() <= 7 && value.bytes().all(|b| b.is_ascii_digit()) }

fn readable(url: &Url) -> bool {
    if !official(url) { return false; }
    let path = url.path().trim_matches('/');
    let parts: Vec<_> = path.split('/').collect();
    let path_ok = match parts.as_slice() {
        ["" | "all-trainers"] => true,
        ["page", number] => page_number(number),
        ["trainer" | "category" | "tag", slug] => segment(slug),
        ["trainer", slug, comments] => segment(slug) && comments.strip_prefix("comment-page-").is_some_and(page_number),
        ["category" | "tag", slug, "page", number] => segment(slug) && page_number(number),
        _ => false,
    };
    if !path_ok || url.path().contains("//") { return false; }
    let mut search = false;
    let mut paged = false;
    url.query_pairs().all(|(key, value)| match key.as_ref() {
        "s" if !search && value.len() <= 512 && !value.chars().any(char::is_control) => { search = true; true }
        "paged" if !paged && page_number(&value) => { paged = true; true }
        _ => false,
    })
}

#[derive(Default)]
struct Session {
    label: String,
    history: Vec<Url>,
    pending: Option<Url>,
    saving: bool,
    saved: Option<PathBuf>,
}
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn with_session<R>(label: &str, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    let mut state = SESSION.lock().ok()?;
    state.as_mut().filter(|s| s.label == label).map(f)
}

fn clear_session(label: &str) {
    if let Ok(mut state) = SESSION.lock() {
        if state.as_ref().is_some_and(|s| s.label == label) { *state = None; }
    }
}

fn label(input: &Value) -> Result<String, String> {
    let id = input["id"].as_str().and_then(|raw| Uuid::parse_str(raw).ok()).ok_or("官网视图编号无效。")?;
    Ok(format!("{PREFIX}{id}"))
}

fn send(app: &AppHandle, label: &str, phase: &str, value: &str) {
    let Some(id) = label.strip_prefix(PREFIX) else { return; };
    let _ = app.emit_to(EventTarget::webview("main"), "releasehub:reader", json!({"id":id,"phase":phase,"value":value}));
}

fn offer(app: &AppHandle, label: &str, url: &Url) -> bool {
    if attachment(url).is_none() { return false; }
    if with_session(label, |s| {
        if s.saving || s.pending.as_ref() == Some(url) { return false; }
        s.pending = Some(url.clone()); true
    }) == Some(true) { send(app, label, "download", url.as_str()); }
    true
}

pub(crate) fn close_active(app: &AppHandle) -> Result<(), String> {
    for (label, view) in app.webviews() {
        if !label.starts_with(PREFIX) { continue; }
        view.close().map_err(|_| "无法关闭风灵月影官网。")?;
        clear_session(&label);
        send(app, &label, "closed", "");
    }
    Ok(())
}

pub(crate) fn open(app: &AppHandle, input: Value, mode: &str, proxy: Option<Url>) -> Result<Value, String> {
    let label = label(&input)?;
    let rect = bounds(&input)?;
    let window = app.get_window("main").ok_or("主窗口未打开。")?;
    super::close_active(app)?;
    close_active(app)?;
    let mut state = SESSION.lock().map_err(|_| "官网视图不可用。")?;
    *state = Some(Session { label: label.clone(), ..Session::default() });
    drop(state);
    let nav_app = app.clone(); let nav_label = label.clone();
    let popup_app = app.clone(); let popup_label = label.clone();
    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(Url::parse(HOME).expect("static homepage")))
        .incognito(true).disable_javascript().devtools(false).disable_drag_drop_handler()
        .on_permission_request(|_, _| PermissionResponse::Deny)
        .on_navigation(move |url| {
            if offer(&nav_app, &nav_label, url) { false }
            else if readable(url) { true }
            else { send(&nav_app, &nav_label, "blocked", ""); false }
        })
        .on_new_window(move |url, _| {
            if !offer(&popup_app, &popup_label, &url) {
                if readable(&url) {
                    if let Some(view) = popup_app.get_webview(&popup_label) { tauri::async_runtime::spawn(async move { let _ = view.navigate(url); }); }
                } else { send(&popup_app, &popup_label, "blocked", ""); }
            }
            NewWindowResponse::Deny
        })
        .on_download(|view, event| {
            if let DownloadEvent::Requested { url, .. } = event {
                if !offer(view.app_handle(), view.label(), &url) { send(view.app_handle(), view.label(), "blocked", ""); }
            }
            false
        })
        .on_document_title_changed(|view, title| { send(view.app_handle(), view.label(), "title", &title.chars().take(160).collect::<String>()); })
        .on_page_load(|view, event| {
            let url = event.url();
            if !readable(url) { return; }
            let ready = event.event() == PageLoadEvent::Finished;
            if with_session(view.label(), |s| {
                if ready {
                    if s.history.last() != Some(url) {
                        if s.history.len() >= 64 { s.history.remove(0); }
                        s.history.push(url.clone());
                    }
                } else if !s.saving { s.pending = None; }
            }).is_some() { send(view.app_handle(), view.label(), if ready { "ready" } else { "loading" }, url.as_str()); }
        });
    if let Some(proxy) = proxy { builder = builder.proxy_url(proxy); }
    else if mode == "direct" { builder = builder.additional_browser_args("--no-proxy-server"); }
    if window.add_child(builder, rect.position, rect.size).is_err() {
        clear_session(&label);
        return Err("无法加载风灵月影官网，请重试或用浏览器打开。".into());
    }
    Ok(Value::Null)
}

pub(crate) fn control(app: &AppHandle, action: &str, input: Value) -> Result<Value, String> {
    let label = label(&input)?;
    let Some(view) = app.get_webview(&label) else {
        if action == "trainer_close" { clear_session(&label); }
        return match action { "trainer_close" | "trainer_resize" => Ok(Value::Null), _ => Err("官网视图已关闭。".into()) };
    };
    match action {
        "trainer_resize" => view.set_bounds(bounds(&input)?).map_err(|_| "无法调整官网视图。")?,
        "trainer_close" => { view.close().map_err(|_| "无法关闭风灵月影官网。")?; clear_session(&label); },
        "trainer_external" => {
            let url = view.url().ok().filter(readable);
            app.opener().open_url(url.as_ref().map_or(HOME, Url::as_str), None::<&str>).map_err(|_| "无法打开系统浏览器。")?;
        }
        "trainer_reveal" => {
            let path = with_session(&label, |s| s.saved.clone()).flatten().ok_or("尚未保存附件。")?;
            let parent = safe_parent(&path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| "该附件已移动或删除。")?;
            if !metadata.is_file() || linked(&metadata) || path.canonicalize().ok().as_deref().and_then(Path::parent) != Some(parent.as_path()) {
                return Err("该附件已移动或路径无效。".into());
            }
            app.opener().reveal_item_in_dir(path).map_err(|_| "无法在文件夹中显示该附件。")?;
        }
        "trainer_navigate" => {
            let mut url = Url::parse(HOME).expect("static homepage");
            match input["location"].as_str() {
                Some("home") => {},
                Some("catalog") => url.set_path("/all-trainers/"),
                Some("search") => {
                    let word = input["query"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)).ok_or("请输入不超过 512 字节的游戏名称。")?;
                    url.query_pairs_mut().append_pair("s", word);
                }
                Some("back") => {
                    let previous = with_session(&label, |s| {
                        if s.history.len() < 2 { return None; }
                        s.history.pop(); s.history.last().cloned()
                    }).flatten();
                    let Some(previous) = previous else { return Ok(Value::Null); };
                    url = previous;
                }
                _ => return Err("未知官网导航操作。".into()),
            }
            view.navigate(url).map_err(|_| "无法打开官网页面。")?;
        }
        _ => return Err("未知官网操作。".into()),
    }
    Ok(Value::Null)
}

fn linked(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return true; } }
    false
}

fn safe_parent(path: &Path) -> Result<PathBuf, String> {
    let invalid = "请选择不含链接的本机文件夹。";
    if !path.is_absolute() { return Err(invalid.into()); }
    let mut components = path.components();
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(components.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))) { return Err(invalid.into()); }
    }
    if !matches!(components.next(), Some(Component::RootDir)) ||
        !components.all(|c| matches!(c, Component::Normal(n) if n.to_str().is_some_and(plain_name))) { return Err(invalid.into()); }
    let parent = path.parent().ok_or(invalid)?;
    for directory in parent.ancestors().filter(|p| p.has_root()) {
        let metadata = fs::symlink_metadata(directory).map_err(|_| invalid)?;
        if !metadata.is_dir() || linked(&metadata) { return Err(invalid.into()); }
    }
    parent.canonicalize().map_err(|_| invalid.into())
}

fn destination(path: &Path, ext: &str) -> Result<PathBuf, String> {
    let file_name = path.file_name().and_then(|value| value.to_str()).filter(|name| plain_name(name) && extension(name, ext)).ok_or("文件名无效，必须保留官网附件的原扩展名。")?;
    let path = safe_parent(path)?.join(file_name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(path),
        Ok(_) => Err("目标文件已存在，追新不会覆盖它。".into()),
        Err(_) => Err("无法确认目标文件路径。".into()),
    }
}

fn single_header(response: &Response, name: header::HeaderName) -> Result<Option<&str>, String> {
    let mut values = response.headers().get_all(name).iter();
    let value = values.next().map(|v| v.to_str().map_err(|_| "官网返回了无效的下载响应。".to_string())).transpose()?;
    if values.next().is_some() { return Err("官网返回了重复的下载响应头。".into()); }
    Ok(value)
}

async fn request(client: &Client, url: &Url) -> Result<Response, String> {
    client.get(url.clone()).header(header::REFERER, HOME).header(header::ACCEPT_ENCODING, "identity").send().await
        .map_err(|e| if e.is_timeout() { "连接风灵月影官网超时。" } else { "无法连接风灵月影官网，请检查网络或代理。" }.into())
}

async fn resolve(client: &Client, mut url: Url) -> Result<(Url, String), String> {
    for _ in 0..3 {
        let current = attachment(&url).ok_or(BAD_LINK)?;
        if let Some(name) = current.file_name() { return Ok((url, name.to_owned())); }
        let response = request(client, &url).await?;
        if !matches!(response.status(), StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT) {
            return Err("官网未返回有效的附件跳转，可在浏览器中重试。".into());
        }
        let location = single_header(&response, header::LOCATION)?.filter(|value| value.len() <= 4096 && !value.chars().any(|c| c == '\\' || c.is_control())).ok_or(BAD_LINK)?;
        let next = url.join(location).map_err(|_| BAD_LINK)?;
        if !attachment(&next).is_some_and(|target| current.follows(&target)) { return Err("已拦截非官网附件跳转。".into()); }
        url = next;
    }
    Err("官网附件跳转次数过多。".into())
}

fn expected_length(response: &Response, ext: &str) -> Result<Option<u64>, String> {
    if response.status() != StatusCode::OK || single_header(response, header::CONTENT_RANGE)?.is_some() ||
        single_header(response, header::CONTENT_ENCODING)?.is_some_and(|s| !s.eq_ignore_ascii_case("identity")) {
        return Err("官网没有返回完整的附件，可在浏览器中重试。".into());
    }
    let format = single_header(response, header::CONTENT_TYPE)?.unwrap_or_default().split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let valid = format == "application/octet-stream" || match ext {
        "exe" => matches!(format.as_str(), "application/x-msdownload" | "application/vnd.microsoft.portable-executable"),
        "zip" => matches!(format.as_str(), "application/zip" | "application/x-zip-compressed"),
        _ => false,
    };
    if !valid { return Err("官网返回的内容不是受支持的附件。".into()); }
    single_header(response, header::CONTENT_LENGTH)?.map(|length| {
        length.parse::<u64>().ok().filter(|n| (4..=MAX_BYTES).contains(n)).ok_or_else(|| "官网附件大小无效或超过 512 MB。".to_owned())
    }).transpose()
}

struct Partial(PathBuf);
impl Drop for Partial { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
struct SaveState(String);
impl Drop for SaveState { fn drop(&mut self) { with_session(&self.0, |s| s.saving = false); } }

async fn transfer(client: &Client, url: &Url, path: &Path, ext: &str, label: &str) -> Result<(), String> {
    let response = request(client, url).await?;
    let length = expected_length(&response, ext)?;
    let parent = safe_parent(path)?;
    if path.parent() != Some(parent.as_path()) || with_session(label, |_| ()).is_none() { return Err(CLOSED.into()); }
    let partial = parent.join(format!(".releasehub-{}.part", Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&partial).await.map_err(|_| "无法创建临时下载文件，请检查磁盘权限。")?;
    let partial = Partial(partial);
    let mut stream = response.bytes_stream();
    let mut count = 0u64;
    let mut magic = Vec::with_capacity(4);
    while let Some(chunk) = stream.next().await {
        if with_session(label, |_| ()).is_none() { return Err(CLOSED.into()); }
        let chunk = chunk.map_err(|_| "下载中断，未保存附件。")?;
        count = count.checked_add(chunk.len() as u64).filter(|n| *n <= length.unwrap_or(MAX_BYTES)).ok_or("下载内容超过允许的大小。")?;
        let needed = (4 - magic.len()).min(chunk.len());
        if needed != 0 {
            magic.extend_from_slice(&chunk[..needed]);
            if magic.len() == 4 && !(if ext == "exe" { magic.starts_with(b"MZ") } else { magic == b"PK\x03\x04" }) {
                return Err("下载内容与附件格式不符，未保存。".into());
            }
        }
        file.write_all(&chunk).await.map_err(|_| "写入失败，请检查磁盘空间。")?;
    }
    if count < 4 || length.is_some_and(|total| total != count) { return Err("下载不完整，未保存。".into()); }
    file.sync_all().await.map_err(|_| "无法保存附件，请检查磁盘空间。")?;
    drop(file);
    if safe_parent(path)? != parent || with_session(label, |_| ()).is_none() { return Err(CLOSED.into()); }
    let metadata = fs::symlink_metadata(&partial.0).map_err(|_| "临时下载文件不可用。")?;
    if !metadata.is_file() || linked(&metadata) { return Err("临时下载文件路径不安全。".into()); }
    // Carry Windows' Internet-zone warning over to the finished file without running it.
    #[cfg(windows)]
    {
        let mut zone = partial.0.as_os_str().to_owned(); zone.push(":Zone.Identifier");
        let mut tag = tokio::fs::OpenOptions::new().write(true).create_new(true).open(PathBuf::from(zone)).await.map_err(|_| "无法标记 Internet 来源，请保存到本机 NTFS 文件夹。")?;
        tag.write_all(b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://flingtrainer.com/\r\n").await.map_err(|_| "无法标记附件的 Internet 来源。")?;
        tag.sync_all().await.map_err(|_| "无法标记附件的 Internet 来源。")?;
    }
    // No replacement, including existing links. Publishing and recording the result are
    // ordered with the view's close, so a closed page cannot finish an invisible download.
    with_session(label, |s| {
        if safe_parent(path)? != parent { return Err("所选文件夹已改变，未保存附件。".to_owned()); }
        fs::hard_link(&partial.0, path).map_err(|_| "无法保存附件，请确认目标未被占用，并使用本机 NTFS 文件夹。".to_owned())?;
        s.saved = Some(path.to_owned());
        s.pending = None;
        Ok(())
    }).ok_or(CLOSED)?
}

pub(crate) async fn save(app: &AppHandle, input: Value, proxy: &ProxyState) -> Result<Value, String> {
    let label = label(&input)?;
    let url = input["url"].as_str().and_then(parse_url).filter(|url| attachment(url).is_some()).ok_or(BAD_LINK)?;
    if app.get_webview(&label).is_none() { return Err(CLOSED.into()); }
    with_session(&label, |s| {
        if s.saving { return Err("已有一个官网附件正在保存。"); }
        if s.pending.as_ref() != Some(&url) { return Err("请先在官网页面中点选此附件。"); }
        s.saving = true; Ok(())
    }).ok_or(CLOSED)??;
    let _saving = SaveState(label.clone());
    let client = proxy.client_builder()?.https_only(true).redirect(reqwest::redirect::Policy::none()).referer(false)
        .user_agent(DOWNLOAD_AGENT).connect_timeout(Duration::from_secs(15)).read_timeout(Duration::from_secs(30)).timeout(Duration::from_secs(600))
        .build().map_err(|_| "无法创建官网下载连接。")?;
    let (file_url, name) = resolve(&client, url).await?;
    if with_session(&label, |_| ()).is_none() { return Err(CLOSED.into()); }
    let ext = if extension(&name, "exe") { "exe" } else { "zip" };
    let app = app.clone();
    let suggested = name;
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().add_filter("风灵月影官网附件", &[ext]).set_file_name(suggested);
        if let Some(parent) = app.get_window("main") { dialog.set_parent(&parent).blocking_save_file() }
        else { None }
    }).await.map_err(|_| "无法打开附件保存对话框。")?;
    let Some(chosen) = chosen else { return Ok(Value::Null); };
    let path = destination(&chosen.into_path().map_err(|_| "请选择本机保存位置。")?, ext)?;
    transfer(&client, &file_url, &path, ext, &label).await?;
    Ok(json!({"path":path,"fileName":path.file_name().and_then(|value| value.to_str()).unwrap_or_default()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_official_chain_keeps_archive_identity() {
        let kind = |value: &str| attachment(&Url::parse(value).unwrap());
        let ticket = kind("https://flingtrainer.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,").unwrap();
        let hop = kind("https://flingtrainer.com/download-trainer.php?path=%2Fwp-content%2Fuploads%2F2024%2F08%2FBlack.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG.zip").unwrap();
        let file = kind("https://flingtrainer.com/wp-content/uploads/trainer-files/2024/08/Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG.zip/Black%20Myth%20Wukong%20v1.0-v1.0.20%20Plus%2044%20Trainer.exe").unwrap();
        let changed = kind("https://flingtrainer.com/wp-content/uploads/trainer-files/2024/08/Other.zip/Black%20Myth%20Wukong.exe").unwrap();
        assert!(ticket.follows(&hop) && hop.follows(&file));
        assert!(!hop.follows(&changed) && !ticket.follows(&file) && !file.follows(&changed));
        assert_eq!(file.file_name(), Some("Black Myth Wukong v1.0-v1.0.20 Plus 44 Trainer.exe"));
        for url in ["https://www.flingtrainer.com/wp-content/uploads/2026/02/official.zip", "https://flingtrainer.com.evil.test/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,", "https://flingtrainer.com:444/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,", "https://guest@flingtrainer.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,", "https://flingtrainer.com/download-trainer.php?path=/wp-content/uploads/2026/01/safe.zip&other=1", "https://flingtrainer.com/download-trainer.php?path=//wemod.com/a.zip", "https://flingtrainer.com/wp-content/uploads/trainer-files/2026/13/other.zip/something.exe", "https://flingtrainer.com/wp-content/uploads/trainer-files/2026/01/other.zip/evil%2Fa.exe", "https://flingtrainer.com/wp-content/uploads/trainer-files/2026/01/other.zip/CON.exe", "https://flingtrainer.com/wp-content/uploads/trainer-files/2026/01/other.zip/abc.exe%3Anote", "https://wemod.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,"] {
            assert!(kind(url).is_none(), "{url}");
        }
    }

    #[test]
    fn only_official_browsing_routes_and_search_are_allowed() {
        let page = |raw: &str| readable(&Url::parse(raw).unwrap());
        assert!(page(HOME));
        assert!(page("https://flingtrainer.com/?s=Black+Myth"));
        assert!(page("https://flingtrainer.com/page/2/?s=Black+Myth"));
        assert!(page("https://flingtrainer.com/all-trainers/"));
        assert!(page("https://flingtrainer.com/trainer/black-myth-wukong-trainer/#comments"));
        assert!(!page("https://flingtrainer.com/?redirect=https://example.test"));
        assert!(!page("https://flingtrainer.com/wp-login.php"));
        assert!(!page("https://api.wemod.com/fling/trainer-page.js?title_id=106087"));
    }

    #[test]
    fn destination_rejects_overwrites_and_name_tricks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("Trainer.exe");
        assert_eq!(destination(&file, "exe").unwrap(), file);
        fs::write(&file, b"existing").unwrap();
        assert!(destination(&file, "exe").is_err());
        for bad in ["NUL.exe", "COM¹.exe", "decoy.lnk", "Trainer.exe:secret", "Trainer.exe."] {
            assert!(destination(&root.join(bad), "exe").is_err(), "{bad}");
        }
    }
}
