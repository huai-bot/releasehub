//! Internal transfer engine. URL policy is supplied by the caller; production always uses
//! network::trusted_download_url. Tests can exercise HTTP without changing that policy.
use super::network;
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::{header, Client, Response, StatusCode};
use sha2::{Digest, Sha256};
use std::{path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicU64, Ordering}, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

const PARALLEL_THRESHOLD: u64 = 4 * 1024 * 1024;
const CANCELLED: &str = "下载已取消";
type UrlPolicy<'a> = &'a (dyn Fn(&Url) -> Result<(), String> + Sync);

pub(super) struct TransferOptions<'a> {
    pub url: &'a str,
    pub expected_size: u64,
    pub connections: usize,
    pub expected_sha256: Option<&'a str>,
}
pub(super) struct TransferResult {
    pub received_bytes: u64,
    pub sha256: String,
}

pub(super) fn linked(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return true; } }
    false
}
fn safe_folder(folder: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::symlink_metadata(folder).map_err(|_| "无法读取下载目录".to_string())?;
    if !meta.is_dir() || linked(&meta) { return Err("下载目录不能是链接".into()); }
    std::fs::canonicalize(folder).map_err(|_| "下载目录无效".into())
}
fn regular_file(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "无法读取临时下载文件".to_string())?;
    if !meta.is_file() || linked(&meta) { return Err("临时下载文件不安全".into()); }
    #[cfg(unix)]
    { use std::os::unix::fs::MetadataExt; if meta.nlink() != 1 { return Err("临时下载文件不能是硬链接".into()); } }
    let parent = path.parent().ok_or("下载目录无效")?;
    if std::fs::canonicalize(path).map_err(|_| "临时下载文件无效".to_string())?.parent() != Some(safe_folder(parent)?.as_path()) {
        return Err("临时下载文件不能离开下载目录".into());
    }
    Ok(())
}
pub(super) async fn prepare_paths(partial: &Path, destination: &Path) -> Result<(), String> {
    let parent = partial.parent().ok_or("下载目录无效")?;
    if destination.parent() != Some(parent) || partial == destination { return Err("下载路径无效".into()); }
    safe_folder(parent)?;
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Err("完整下载文件已存在，请勿覆盖".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err("无法读取完整下载文件路径".into()),
    }
    match std::fs::symlink_metadata(partial) {
        Ok(_) => { regular_file(partial)?; tokio::fs::remove_file(partial).await.map_err(|_| "无法清理中断的下载".to_string())?; },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err("无法读取临时下载文件路径".into()),
    }
    Ok(())
}
pub(super) async fn publish_partial(partial: &Path, destination: &Path) -> Result<(), String> {
    if partial.parent() != destination.parent() || partial == destination { return Err("下载路径无效".into()); }
    regular_file(partial)?;
    // Atomic no-replace publication; existing files, including dangling links, cause failure.
    tokio::fs::hard_link(partial, destination).await.map_err(|_| "无法保存完整文件，目标可能已存在".into())
}
pub(super) async fn remove_partial(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => { regular_file(path)?; tokio::fs::remove_file(path).await.map_err(|_| "无法清理临时下载文件".into()) },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法读取临时下载文件".into()),
    }
}

async fn wait_stop(cancel: &AtomicBool, stop: &AtomicBool) {
    loop {
        if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) { return; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
fn check_stop(cancel: &AtomicBool, stop: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
}
async fn send(client: &Client, url: &Url, range: Option<(u64, u64)>, validator: Option<&str>, cancel: &AtomicBool, stop: &AtomicBool, allow: UrlPolicy<'_>) -> Result<Response, String> {
    check_stop(cancel, stop)?;
    allow(url)?;
    let mut request = client.get(url.clone()).header(header::ACCEPT_ENCODING, "identity");
    if let Some((start, end)) = range { request = request.header(header::RANGE, format!("bytes={start}-{end}")); }
    if let Some(value) = validator { request = request.header(header::IF_RANGE, value); }
    let response = tokio::select! {
        response = request.send() => response.map_err(|e| network::request_error(&e))?,
        _ = wait_stop(cancel, stop) => return Err(CANCELLED.into()),
    };
    allow(response.url())?;
    Ok(response)
}
fn field(response: &Response, name: header::HeaderName) -> Result<Option<String>, String> {
    let mut values = response.headers().get_all(name).iter();
    let value = values.next().map(|v| v.to_str().map(str::to_owned).map_err(|_| "下载响应头格式无效".to_string())).transpose()?;
    if values.next().is_some() { return Err("下载响应头重复，已拒绝此文件".into()); }
    Ok(value)
}
fn encoding(response: &Response) -> Result<(), String> {
    if field(response, header::CONTENT_ENCODING)?.is_some_and(|s| !s.eq_ignore_ascii_case("identity")) {
        return Err("下载响应使用了非 identity 编码".into());
    }
    Ok(())
}
fn length(response: &Response) -> Result<Option<u64>, String> {
    field(response, header::CONTENT_LENGTH)?.map(|v| {
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) { return Err("下载 Content-Length 无效".into()); }
        v.parse().map_err(|_| "下载 Content-Length 无效".into())
    }).transpose()
}
fn exact_range(response: &Response, start: u64, end: u64, total: u64) -> Result<(), String> {
    encoding(response)?;
    if response.status() != StatusCode::PARTIAL_CONTENT
        || field(response, header::CONTENT_RANGE)?.as_deref() != Some(format!("bytes {start}-{end}/{total}").as_str())
        || length(response)? != Some(end - start + 1) {
        return Err("下载分段的 Content-Range 或 Content-Length 不符".into());
    }
    Ok(())
}
#[derive(Clone)]
struct Identity { etag: Option<String>, modified: Option<String>, validator: Option<String> }
impl Identity {
    fn from_response(response: &Response) -> Result<Self, String> {
        let etag = field(response, header::ETAG)?;
        let modified = field(response, header::LAST_MODIFIED)?;
        let strong = etag.as_ref().filter(|s| s.starts_with('"') && s.ends_with('"') && s.len() >= 2 && s[1..s.len()-1].bytes().all(|b| b >= 0x21 && b != b'"' && b != 0x7f));
        let date = modified.as_ref().filter(|s| chrono::DateTime::parse_from_rfc2822(s).is_ok());
        let validator = strong.or(date).cloned();
        Ok(Self { etag, modified, validator })
    }
    fn matches(&self, response: &Response) -> Result<(), String> {
        let other = Self::from_response(response)?;
        if self.etag != other.etag || self.modified != other.modified { return Err("下载附件 identity 已变更，请重新检查更新".into()); }
        Ok(())
    }
}
fn status_error(response: &Response) -> String {
    match response.status().as_u16() {
        404 => "发行附件已删除，请重新检查更新",
        403 | 429 => "GitHub 暂时限制下载，请稍后重试",
        _ => "附件下载失败，请稍后重试",
    }.into()
}
async fn consume(response: Response, file: &mut tokio::fs::File, expected: u64, cancel: &AtomicBool, stop: &AtomicBool, received: &AtomicU64, mut hash: Option<&mut Sha256>) -> Result<(), String> {
    let mut stream = response.bytes_stream();
    let mut count = 0u64;
    loop {
        let chunk = tokio::select! {
            chunk = tokio::time::timeout(Duration::from_secs(60), stream.next()) => chunk.map_err(|_| "下载连接长时间没有响应".to_string())?,
            _ = wait_stop(cancel, stop) => return Err(CANCELLED.into()),
        };
        let Some(chunk) = chunk else { break; };
        let chunk = chunk.map_err(|e| network::request_error(&e))?;
        check_stop(cancel, stop)?;
        count = count.checked_add(chunk.len() as u64).ok_or("下载大小超限")?;
        if count > expected { return Err("下载内容超过附件声明大小".into()); }
        file.write_all(&chunk).await.map_err(|_| "下载写入失败，请检查磁盘空间".to_string())?;
        if let Some(hash) = hash.as_deref_mut() { hash.update(&chunk); }
        received.fetch_add(chunk.len() as u64, Ordering::AcqRel);
    }
    check_stop(cancel, stop)?;
    if count != expected { return Err("下载不完整，请重试".into()); }
    file.flush().await.map_err(|_| "无法保存下载文件".to_string())?;
    Ok(())
}
// Polling all borrowed futures starts actual simultaneous HTTP streams. On any failure,
// set stop and drain every future, so file handles close before fragments are removed.
async fn monitor<F: std::future::Future<Output = Result<(), String>>>(future: F, received: &AtomicU64, cancel: &AtomicBool, stop: &AtomicBool, progress: &mut (dyn FnMut(u64) -> Result<(), String> + Send)) -> Result<(), String> {
    tokio::pin!(future);
    let mut ticks = tokio::time::interval(Duration::from_millis(400));
    let mut callback_error = None;
    loop {
        tokio::select! {
            result = &mut future => { return callback_error.map_or(result, Err); },
            _ = ticks.tick(), if callback_error.is_none() => {
                if cancel.load(Ordering::Acquire) { stop.store(true, Ordering::Release); }
                if let Err(e) = progress(received.load(Ordering::Acquire)) { callback_error = Some(e); stop.store(true, Ordering::Release); }
            },
        }
    }
}
struct Fragments { directory: PathBuf, files: Vec<PathBuf>, parent: PathBuf }
impl Fragments {
    async fn create(parent: &Path, count: usize) -> Result<Self, String> {
        let resolved = safe_folder(parent)?;
        let directory = parent.join(format!(".transfer-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&directory).await.map_err(|_| "无法创建下载分片目录".to_string())?;
        if safe_folder(&directory)?.parent() != Some(resolved.as_path()) { return Err("下载分片目录不安全".into()); }
        let files = (0..count).map(|n| directory.join(format!("{n}.part"))).collect();
        Ok(Self { directory, files, parent: resolved })
    }
    async fn cleanup(&self) -> Result<(), String> {
        if safe_folder(&self.directory)?.parent() != Some(self.parent.as_path()) { return Err("下载分片目录不安全".into()); }
        // Exact generated paths only, never recursive deletion or traversal of a link.
        for path in &self.files { remove_partial(path).await?; }
        tokio::fs::remove_dir(&self.directory).await.map_err(|_| "无法清理下载分片目录".into())
    }
}
enum SegmentError { Fallback, Failed(String), Stopped }
async fn segment(client: &Client, url: &Url, start: u64, end: u64, total: u64, identity: &Identity, path: &Path, cancel: &AtomicBool, stop: &AtomicBool, received: &AtomicU64, allow: UrlPolicy<'_>) -> Result<(), SegmentError> {
    let result: Result<bool, String> = async {
        let response = send(client, url, Some((start, end)), identity.validator.as_deref(), cancel, stop, allow).await?;
        identity.matches(&response)?;
        if response.status() == StatusCode::OK { encoding(&response)?; return Ok(false); }
        if !response.status().is_success() { return Err(status_error(&response)); }
        exact_range(&response, start, end, total)?;
        safe_folder(path.parent().ok_or("下载目录无效")?)?;
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(path).await.map_err(|_| "无法创建下载分片".to_string())?;
        consume(response, &mut file, end - start + 1, cancel, stop, received, None).await?;
        Ok(true)
    }.await;
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(SegmentError::Fallback),
        Err(e) if e == CANCELLED && stop.load(Ordering::Acquire) => Err(SegmentError::Stopped),
        Err(e) => Err(SegmentError::Failed(e)),
    }
}
async fn parallel(client: &Client, url: &Url, total: u64, identity: &Identity, fragments: &Fragments, cancel: &AtomicBool, stop: &AtomicBool, received: &AtomicU64, allow: UrlPolicy<'_>) -> Result<bool, String> {
    let mut tasks = FuturesUnordered::new();
    let count = fragments.files.len() as u64;
    for (index, path) in fragments.files.iter().enumerate() {
        let start = total * index as u64 / count;
        let end = total * (index as u64 + 1) / count - 1;
        tasks.push(segment(client, url, start, end, total, identity, path, cancel, stop, received, allow));
    }
    let mut fallback = false;
    let mut error = None;
    while let Some(result) = tasks.next().await {
        match result {
            Ok(()) | Err(SegmentError::Stopped) => {},
            Err(SegmentError::Fallback) => { fallback = true; stop.store(true, Ordering::Release); },
            Err(SegmentError::Failed(e)) => { if error.is_none() { error = Some(e); } stop.store(true, Ordering::Release); },
        }
    }
    if cancel.load(Ordering::Acquire) { return Err(CANCELLED.into()); }
    if let Some(e) = error { return Err(e); }
    Ok(!fallback)
}
async fn merge(file: &mut tokio::fs::File, fragments: &Fragments, total: u64, cancel: &AtomicBool, stop: &AtomicBool) -> Result<Sha256, String> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 128 * 1024];
    for path in &fragments.files {
        regular_file(path)?;
        let mut fragment = tokio::fs::File::open(path).await.map_err(|_| "无法读取下载分片".to_string())?;
        loop {
            check_stop(cancel, stop)?;
            let count = fragment.read(&mut buffer).await.map_err(|_| "读取下载分片失败".to_string())?;
            if count == 0 { break; }
            bytes += count as u64;
            if bytes > total { return Err("下载分片合并大小超限".into()); }
            file.write_all(&buffer[..count]).await.map_err(|_| "合并下载分片失败，请检查磁盘空间".to_string())?;
            hash.update(&buffer[..count]);
        }
    }
    if bytes != total { return Err("下载分片合并不完整".into()); }
    Ok(hash)
}
async fn single(response: Response, file: &mut tokio::fs::File, total: u64, identity: Option<&Identity>, cancel: &AtomicBool, stop: &AtomicBool, received: &AtomicU64, progress: &mut (dyn FnMut(u64) -> Result<(), String> + Send)) -> Result<Sha256, String> {
    if response.status() != StatusCode::OK { return Err(status_error(&response)); }
    encoding(&response)?;
    if let Some(identity) = identity { identity.matches(&response)?; }
    if field(&response, header::CONTENT_RANGE)?.is_some() || length(&response)?.is_some_and(|n| n != total) { return Err("下载大小与发行附件记录不符，请重新检查更新".into()); }
    let mut hash = Sha256::new();
    monitor(consume(response, file, total, cancel, stop, received, Some(&mut hash)), received, cancel, stop, progress).await?;
    Ok(hash)
}

pub(super) async fn transfer(client: &Client, options: TransferOptions<'_>, partial: &Path, cancel: &AtomicBool, allow: UrlPolicy<'_>, progress: &mut (dyn FnMut(u64) -> Result<(), String> + Send)) -> Result<TransferResult, String> {
    if options.expected_size == 0 || options.expected_size > network::MAX_DOWNLOAD_BYTES { return Err("发行附件大小无效或超过4GB限制".into()); }
    let stop = AtomicBool::new(false);
    check_stop(cancel, &stop)?;
    let url = Url::parse(options.url).map_err(|_| "下载地址格式无效".to_string())?;
    allow(&url)?;
    let parent = partial.parent().ok_or("下载目录无效")?;
    safe_folder(parent)?;
    // create_new does not follow a pre-existing symlink or overwrite another file.
    let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(partial).await.map_err(|_| "无法创建下载文件，请检查空间和目录权限".to_string())?;
    let received = AtomicU64::new(0);
    let mut fragments = None;
    let result = async {
        let count = options.connections.clamp(1, 8);
        let mut response = send(client, &url, (count > 1 && options.expected_size >= PARALLEL_THRESHOLD).then_some((0, 0)), None, cancel, &stop, allow).await?;
        let mut identity = None;
        let mut range_url = url.clone();
        let hash = if count > 1 && options.expected_size >= PARALLEL_THRESHOLD && response.status() == StatusCode::PARTIAL_CONTENT {
            exact_range(&response, 0, 0, options.expected_size)?;
            let probed = Identity::from_response(&response)?;
            range_url = response.url().clone();
            // Validate the one-byte probe body too, including truncation and overrun.
            let mut stream = response.bytes_stream();
            let mut probe_bytes = 0usize;
            loop {
                let chunk = tokio::select! {
                    chunk = tokio::time::timeout(Duration::from_secs(60), stream.next()) => chunk.map_err(|_| "下载连接长时间没有响应".to_string())?,
                    _ = wait_stop(cancel, &stop) => return Err(CANCELLED.into()),
                };
                let Some(chunk) = chunk else { break; };
                probe_bytes += chunk.map_err(|e| network::request_error(&e))?.len();
                if probe_bytes > 1 { return Err("下载探测分段大小不符".into()); }
            }
            if probe_bytes != 1 { return Err("下载探测分段不完整".into()); }
            identity = Some(probed);
            let mut completed = false;
            if identity.as_ref().and_then(|i| i.validator.as_ref()).is_some() {
                fragments = Some(Fragments::create(parent, count).await?);
                let mut outcome = false;
                let future = async {
                    outcome = parallel(client, &range_url, options.expected_size, identity.as_ref().unwrap(), fragments.as_ref().unwrap(), cancel, &stop, &received, allow).await?;
                    Ok(())
                };
                monitor(future, &received, cancel, &stop, progress).await?;
                completed = outcome;
            }
            if completed {
                merge(&mut file, fragments.as_ref().unwrap(), options.expected_size, cancel, &stop).await?
            } else {
                if let Some(parts) = fragments.take() { parts.cleanup().await?; }
                check_stop(cancel, &AtomicBool::new(false))?;
                stop.store(false, Ordering::Release);
                received.store(0, Ordering::Release);
                progress(0)?;
                response = send(client, &range_url, None, None, cancel, &stop, allow).await?;
                single(response, &mut file, options.expected_size, identity.as_ref(), cancel, &stop, &received, progress).await?
            }
        } else {
            if matches!(response.status().as_u16(), 405 | 416 | 501) && count > 1 && options.expected_size >= PARALLEL_THRESHOLD {
                drop(response);
                response = send(client, &range_url, None, None, cancel, &stop, allow).await?;
            }
            single(response, &mut file, options.expected_size, identity.as_ref(), cancel, &stop, &received, progress).await?
        };
        check_stop(cancel, &stop)?;
        file.flush().await.map_err(|_| "无法保存下载文件".to_string())?;
        file.sync_all().await.map_err(|_| "无法保存下载文件".to_string())?;
        let sha256 = format!("{:x}", hash.finalize());
        if options.expected_sha256.is_some_and(|digest| digest != sha256) { return Err("SHA-256 校验不符，已拒绝此文件".into()); }
        progress(received.load(Ordering::Acquire))?;
        check_stop(cancel, &stop)?;
        Ok(TransferResult { received_bytes: received.load(Ordering::Acquire), sha256 })
    }.await;
    drop(file);
    let cleanup = if let Some(parts) = fragments { parts.cleanup().await } else { Ok(()) };
    let result = match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}；{cleanup}")),
    };
    if result.is_err() {
        if let Err(cleanup) = remove_partial(partial).await { return Err(format!("{}；{cleanup}", result.err().unwrap())); }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, atomic::AtomicUsize};
    use tokio::{net::{TcpListener, TcpStream}, task::JoinSet};

    #[derive(Clone, Copy, PartialEq)]
    enum Mode { Good, Ignore, NoIdentity, LateIgnore, Truncated, Changed, ChangedDate, BadRange, BadLength, Encoded, Redirect }
    struct State {
        mode: Mode,
        data: Vec<u8>,
        active: AtomicUsize,
        peak: AtomicUsize,
        ranges: Mutex<Vec<(u64, u64)>>,
        full: AtomicUsize,
        invalid_if_range: AtomicBool,
    }
    struct Active(Arc<State>);
    impl Drop for Active { fn drop(&mut self) { self.0.active.fetch_sub(1, Ordering::AcqRel); } }
    struct Fixture { url: String, state: Arc<State>, task: tokio::task::JoinHandle<()> }
    impl Drop for Fixture { fn drop(&mut self) { self.task.abort(); } }
    impl Fixture {
        async fn start(mode: Mode, size: usize) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/asset", listener.local_addr().unwrap());
            let state = Arc::new(State { mode, data: (0..size).map(|i| (i % 251) as u8).collect(), active: AtomicUsize::new(0), peak: AtomicUsize::new(0), ranges: Mutex::new(Vec::new()), full: AtomicUsize::new(0), invalid_if_range: AtomicBool::new(false) });
            let server = state.clone();
            let task = tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let Ok((socket, _)) = accepted else { break; };
                            let state = server.clone();
                            connections.spawn(async move { let _ = serve(socket, state).await; });
                        },
                        _ = connections.join_next(), if !connections.is_empty() => {},
                    }
                }
            });
            Self { url, state, task }
        }
        fn client(&self) -> Client { Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap() }
    }
    async fn serve(mut socket: TcpStream, state: Arc<State>) -> Result<(), std::io::Error> {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|s| s == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).await?;
            if n == 0 { return Ok(()); }
            request.extend_from_slice(&buffer[..n]);
            if request.len() > 16384 { return Ok(()); }
        }
        let text = String::from_utf8(request).unwrap();
        if state.mode == Mode::Redirect && text.starts_with("GET /asset ") {
            socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /blocked\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
            return Ok(());
        }
        let range = text.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_owned)).and_then(|value| {
            let (a, b) = value.split_once('-')?;
            Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
        });
        if let Some(range) = range { state.ranges.lock().unwrap().push(range); }
        let segment = range.is_some_and(|r| r != (0, 0));
        if segment && !text.to_ascii_lowercase().contains("if-range: \"fixture-v1\"") { state.invalid_if_range.store(true, Ordering::Release); }
        let range_supported = state.mode != Mode::Ignore && !(state.mode == Mode::LateIgnore && segment);
        let (start, end, status) = if range_supported {
            if let Some((start, end)) = range { (start, end, "206 Partial Content") } else { (0, state.data.len() as u64 - 1, "200 OK") }
        } else { (0, state.data.len() as u64 - 1, "200 OK") };
        let _active = if segment {
            let active = state.active.fetch_add(1, Ordering::AcqRel) + 1;
            state.peak.fetch_max(active, Ordering::AcqRel);
            Some(Active(state.clone()))
        } else { None };
        if status == "200 OK" { state.full.fetch_add(1, Ordering::AcqRel); }
        let size = end - start + 1;
        let mut headers = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n", if segment && state.mode == Mode::BadLength { size + 1 } else { size });
        if status.starts_with("206") { headers.push_str(&format!("Content-Range: bytes {}-{end}/{}\r\n", if segment && state.mode == Mode::BadRange { start + 1 } else { start }, state.data.len())); }
        if state.mode != Mode::NoIdentity {
            headers.push_str(if segment && state.mode == Mode::Changed { "ETag: \"fixture-v2\"\r\n" } else { "ETag: \"fixture-v1\"\r\n" });
            headers.push_str(if segment && state.mode == Mode::ChangedDate { "Last-Modified: Thu, 19 Nov 2026 00:00:00 GMT\r\n" } else { "Last-Modified: Wed, 18 Nov 2026 00:00:00 GMT\r\n" });
        }
        if segment && state.mode == Mode::Encoded { headers.push_str("Content-Encoding: gzip\r\n"); }
        headers.push_str("\r\n");
        socket.write_all(headers.as_bytes()).await?;
        // Keep real segment sockets simultaneously open, rather than merely counting starts.
        if segment { tokio::time::sleep(Duration::from_millis(120)).await; }
        let body = &state.data[start as usize..=end as usize];
        if segment && state.mode == Mode::Truncated { socket.write_all(&body[..body.len() / 2]).await?; }
        else { socket.write_all(body).await?; }
        socket.shutdown().await
    }
    fn allow_fixture(url: &Url) -> Result<(), String> {
        if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.path() == "/asset" { Ok(()) } else { Err("不可信 fixture 地址".into()) }
    }
    async fn run(fixture: &Fixture, partial: &Path, connections: usize, digest: Option<&str>, cancel: &AtomicBool, progress: &mut (dyn FnMut(u64) -> Result<(), String> + Send)) -> Result<TransferResult, String> {
        transfer(&fixture.client(), TransferOptions { url: &fixture.url, expected_size: fixture.state.data.len() as u64, connections, expected_sha256: digest }, partial, cancel, &allow_fixture, progress).await
    }
    fn only_file(dir: &Path, name: &str) {
        let paths: Vec<_> = std::fs::read_dir(dir).unwrap().map(|p| p.unwrap().file_name()).collect();
        assert_eq!(paths, vec![std::ffi::OsString::from(name)]);
    }
    #[tokio::test]
    async fn actual_parallel_ranges_merge_hash_and_aggregate_progress() {
        for connections in [2, 3, 4, 8] {
            let fixture = Fixture::start(Mode::Good, PARALLEL_THRESHOLD as usize + 13).await;
            let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
            let digest = format!("{:x}", Sha256::digest(&fixture.state.data));
            let mut progress = Vec::new();
            let result = run(&fixture, &partial, connections, Some(&digest), &AtomicBool::new(false), &mut |n| { progress.push(n); Ok(()) }).await.unwrap();
            assert_eq!(fixture.state.peak.load(Ordering::Acquire), connections);
            assert!(!fixture.state.invalid_if_range.load(Ordering::Acquire));
            assert_eq!(result.sha256, digest);
            assert_eq!(result.received_bytes, fixture.state.data.len() as u64);
            assert_eq!(std::fs::read(&partial).unwrap(), fixture.state.data);
            assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
            assert_eq!(progress.last(), Some(&result.received_bytes));
            let ranges = fixture.state.ranges.lock().unwrap();
            assert_eq!(ranges.len(), connections + 1);
            assert_eq!(fixture.state.full.load(Ordering::Acquire), 0);
            only_file(dir.path(), "asset.part");
        }
    }
    #[tokio::test]
    async fn unsupported_ranges_and_missing_identity_fall_back_without_fragments() {
        for mode in [Mode::Ignore, Mode::NoIdentity, Mode::LateIgnore] {
            let fixture = Fixture::start(mode, PARALLEL_THRESHOLD as usize + 7).await;
            let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
            let result = run(&fixture, &partial, 4, None, &AtomicBool::new(false), &mut |_| Ok(())).await.unwrap();
            assert_eq!(result.received_bytes, fixture.state.data.len() as u64);
            assert_eq!(std::fs::read(&partial).unwrap(), fixture.state.data);
            assert!(fixture.state.full.load(Ordering::Acquire) >= 1);
            if mode != Mode::LateIgnore { assert_eq!(fixture.state.peak.load(Ordering::Acquire), 0); }
            only_file(dir.path(), "asset.part");
        }
    }
    #[tokio::test]
    async fn small_files_and_one_connection_do_not_probe() {
        for (size, connections) in [(123, 8), (PARALLEL_THRESHOLD as usize + 9, 1)] {
            let fixture = Fixture::start(Mode::Good, size).await;
            let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
            run(&fixture, &partial, connections, None, &AtomicBool::new(false), &mut |_| Ok(())).await.unwrap();
            assert!(fixture.state.ranges.lock().unwrap().is_empty());
            assert_eq!(fixture.state.full.load(Ordering::Acquire), 1);
            only_file(dir.path(), "asset.part");
        }
    }
    #[tokio::test]
    async fn truncated_changed_identity_invalid_headers_and_digest_remove_every_fragment() {
        for mode in [Mode::Truncated, Mode::Changed, Mode::ChangedDate, Mode::BadRange, Mode::BadLength, Mode::Encoded, Mode::Good] {
            let fixture = Fixture::start(mode, PARALLEL_THRESHOLD as usize + 13).await;
            let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
            let wrong = "0".repeat(64);
            let error = run(&fixture, &partial, 4, (mode == Mode::Good).then_some(wrong.as_str()), &AtomicBool::new(false), &mut |_| Ok(())).await.err().unwrap();
            if mode == Mode::Good { assert!(error.contains("SHA-256")); }
            if mode == Mode::Changed || mode == Mode::ChangedDate { assert!(error.contains("identity")); }
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "{error}");
        }
    }
    #[tokio::test]
    async fn cancellation_stops_open_segments_and_cleans_files() {
        let fixture = Fixture::start(Mode::Good, PARALLEL_THRESHOLD as usize + 13).await;
        let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
        let cancel = AtomicBool::new(false);
        let mut progress = |_| Ok(());
        let transfer = run(&fixture, &partial, 4, None, &cancel, &mut progress);
        let trigger = async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while fixture.state.peak.load(Ordering::Acquire) < 4 { tokio::task::yield_now().await; }
            }).await.unwrap();
            cancel.store(true, Ordering::Release);
        };
        let (result, ()) = tokio::join!(transfer, trigger);
        assert_eq!(result.err().unwrap(), CANCELLED);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[tokio::test]
    async fn final_url_callback_and_existing_destination_are_enforced() {
        let fixture = Fixture::start(Mode::Redirect, 123).await;
        let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
        let error = run(&fixture, &partial, 4, None, &AtomicBool::new(false), &mut |_| Ok(())).await.err().unwrap();
        assert_eq!(error, "不可信 fixture 地址");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(network::trusted_download_url(&fixture.url).is_err());
        let destination = dir.path().join("asset");
        std::fs::write(&destination, b"existing").unwrap();
        assert!(prepare_paths(&partial, &destination).await.is_err());
        assert_eq!(std::fs::read(destination).unwrap(), b"existing");
    }
    #[tokio::test]
    async fn atomic_publication_refuses_overwrite_and_cancelled_transfer_cannot_publish() {
        let fixture = Fixture::start(Mode::Good, 123).await;
        let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part"); let destination = dir.path().join("asset");
        run(&fixture, &partial, 4, None, &AtomicBool::new(false), &mut |_| Ok(())).await.unwrap();
        std::fs::write(&destination, b"existing").unwrap();
        assert!(publish_partial(&partial, &destination).await.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
        std::fs::remove_file(&destination).unwrap();
        publish_partial(&partial, &destination).await.unwrap();
        std::fs::remove_file(&partial).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), fixture.state.data);
        let cancelled = dir.path().join("cancelled.part");
        assert!(run(&fixture, &cancelled, 4, None, &AtomicBool::new(true), &mut |_| Ok(())).await.is_err());
        assert!(publish_partial(&cancelled, &dir.path().join("cancelled")).await.is_err());
    }
    #[tokio::test]
    async fn progress_failure_stops_transfer_and_cleans_fragments() {
        let fixture = Fixture::start(Mode::Good, PARALLEL_THRESHOLD as usize + 13).await;
        let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
        let error = run(&fixture, &partial, 4, None, &AtomicBool::new(false), &mut |_| Err("durable state cancelled".into())).await.err().unwrap();
        assert_eq!(error, "durable state cancelled");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[tokio::test]
    async fn create_new_never_overwrites_existing_partial() {
        let fixture = Fixture::start(Mode::Good, 123).await;
        let dir = tempfile::tempdir().unwrap(); let partial = dir.path().join("asset.part");
        std::fs::write(&partial, b"existing").unwrap();
        assert!(run(&fixture, &partial, 4, None, &AtomicBool::new(false), &mut |_| Ok(())).await.is_err());
        assert_eq!(std::fs::read(&partial).unwrap(), b"existing");
        assert_eq!(fixture.state.full.load(Ordering::Acquire), 0);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn linked_partial_and_linked_directory_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap(); let outside = tempfile::tempdir().unwrap();
        let partial = dir.path().join("asset.part"); let destination = dir.path().join("asset");
        let external = outside.path().join("original"); std::fs::write(&external, b"original").unwrap();
        symlink(&external, &partial).unwrap();
        assert!(prepare_paths(&partial, &destination).await.is_err());
        assert_eq!(std::fs::read(&external).unwrap(), b"original");
        std::fs::remove_file(&partial).unwrap();
        std::fs::hard_link(&external, &partial).unwrap();
        assert!(prepare_paths(&partial, &destination).await.is_err());
        let linked_dir = dir.path().join("linked"); symlink(outside.path(), &linked_dir).unwrap();
        assert!(prepare_paths(&linked_dir.join("p"), &linked_dir.join("d")).await.is_err());
    }
}
