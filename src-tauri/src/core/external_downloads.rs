use super::{downloads::{directory, expected_sha, safe_name, local_file_exists}, models::*, network, Core};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{net::{Ipv4Addr, Ipv6Addr, SocketAddr}, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

const TOKENS: [&str; 7] = ["{url}", "{pageUrl}", "{fileName}", "{projectName}", "{tag}", "{referer}", "{userAgent}"];
const MAX_REQUEST_BYTES: usize = 128 * 1024;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;
const UNKNOWN_RECEIPT: &str = "未确认外部下载器是否接收；请先检查下载器任务，不要直接重发。";

// Construct only from a public source's verified metadata, never from IPC or account state.
#[derive(Clone, Copy)]
pub(crate) struct PublicDownload<'a> {
    pub(crate) url: &'a str,
    pub(crate) page_url: &'a str,
    pub(crate) file_name: &'a str,
    pub(crate) project_name: &'a str,
    pub(crate) tag: &'a str,
    pub(crate) referer: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub(crate) enum ExternalSendOutcome { Sent, Unknown }
fn endpoint(value: &str) -> Result<Url, String> {
    if value.len() > 4096 || value != value.trim() || value.contains('\\') || value.chars().any(char::is_control)
        || value.split_once("://").is_none_or(|(_, rest)| rest.split('/').next().is_none_or(|authority| authority.is_empty() || authority.contains('@'))) {
        return Err("外部下载器接口地址无效，不能包含登录凭据。".into());
    }
    let url = Url::parse(value).map_err(|_| "外部下载器接口地址无效")?;
    if url.scheme() != "http" || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
        || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || url.query().is_some()
        || url.port_or_known_default().is_none_or(|p| p < 1024) {
        return Err("外部下载器接口须使用本机 HTTP 地址和 1024 以上端口，不能含凭据或查询参数".into());
    }
    Ok(url)
}

fn local_addresses(url: &Url) -> Vec<SocketAddr> {
    let port = url.port().unwrap_or(80);
    match url.host_str() {
        Some("localhost") => vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port)), SocketAddr::from((Ipv6Addr::LOCALHOST, port))],
        Some("127.0.0.1") => vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))],
        Some("[::1]") => vec![SocketAddr::from((Ipv6Addr::LOCALHOST, port))],
        _ => Vec::new(),
    }
}
fn validate_template(value: &Value, depth: usize, has_url: &mut bool) -> Result<(), String> {
    if depth > 16 { return Err("接口请求模板层级过多".into()); }
    match value {
        Value::String(s) => {
            if s.contains("{url}") { *has_url = true; }
            let mut rest = s.clone(); for token in TOKENS { rest = rest.replace(token, ""); }
            if rest.contains(['{', '}']) { return Err("模板包含不支持的占位符，可用：{url}、{pageUrl}、{fileName}、{projectName}、{tag}、{referer}、{userAgent}".into()); }
        }
        Value::Array(items) => for item in items { validate_template(item, depth + 1, has_url)?; },
        Value::Object(items) => for (key, item) in items {
            if key.contains(['{', '}']) { return Err("模板字段名不能使用占位符".into()); }
            validate_template(item, depth + 1, has_url)?;
        },
        _ => {}
    }
    Ok(())
}
pub(crate) fn validate_config(config: &ExternalDownloader) -> Result<(), String> {
    if config.name.trim().is_empty() || config.name.chars().count() > 80 || config.name.chars().any(char::is_control) {
        return Err("下载器名称须为 1 至 80 个字符".into());
    }
    endpoint(&config.endpoint)?;
    if config.request_template.len() > 16 * 1024 { return Err("接口请求模板超过 16KB".into()); }
    let value: Value = serde_json::from_str(&config.request_template).map_err(|_| "接口请求模板须为有效 JSON")?;
    if !value.is_object() && !value.is_array() { return Err("接口请求模板须为 JSON 对象或数组".into()); }
    let mut has_url = false; validate_template(&value, 0, &mut has_url)?;
    if !has_url { return Err("接口请求模板须包含 {url} 下载链接占位符".into()); }
    Ok(())
}
fn ab_add(config: &ExternalDownloader, url: &Url) -> bool {
    config.name == "AB Download Manager" && url.path() == "/add"
}

fn request_body(config: &ExternalDownloader, download: PublicDownload<'_>, ab: bool) -> Result<Value, String> {
    validate_config(config)?;
    let mut value: Value = serde_json::from_str(&config.request_template).map_err(|_| "接口请求模板无效")?;
    let stock_ab = ab && serde_json::from_str::<Value>(&ExternalDownloader::default().request_template).ok().as_ref() == Some(&value);
    let agent = if download.referer.is_some() { crate::reader::trainer::DOWNLOAD_AGENT } else { "" };
    fn fill(value: &mut Value, replacements: &[(&str, &str)], budget: &mut usize) -> Result<(), String> {
        match value {
            Value::String(s) => {
                // Replace tokens in the original string only. Data containing tokens is never interpreted a second time.
                let mut out = String::new(); let mut rest = s.as_str();
                while let Some((at, token, replacement)) = replacements.iter().filter_map(|(token, replacement)| rest.find(token).map(|at| (at, *token, *replacement))).min_by_key(|item| item.0) {
                    *budget = budget.checked_sub(replacement.len()).ok_or("生成的下载器请求过大，请精简接口模板。")?;
                    out.push_str(&rest[..at]); out.push_str(replacement); rest = &rest[at + token.len()..];
                }
                out.push_str(rest); *s = out;
            }
            Value::Array(items) => for item in items { fill(item, replacements, budget)?; },
            Value::Object(items) => for item in items.values_mut() { fill(item, replacements, budget)?; },
            _ => {}
        }
        Ok(())
    }
    let mut budget = MAX_REQUEST_BYTES;
    fill(&mut value, &[("{url}", download.url), ("{pageUrl}", download.page_url), ("{fileName}", download.file_name),
        ("{projectName}", download.project_name), ("{tag}", download.tag), ("{referer}", download.referer.unwrap_or("")), ("{userAgent}", agent)], &mut budget)?;
    if let Some(referer) = download.referer.filter(|_| stock_ab) {
        value["items"][0]["headers"] = json!({"Referer": referer, "User-Agent": agent});
    }
    Ok(value)
}

fn public_https(raw: &str) -> Option<Url> {
    if raw.len() > 4096 || !raw.starts_with("https://") || raw.contains('\\') || raw.chars().any(char::is_control)
        || raw[8..].split('/').next()?.contains('@') { return None; }
    let url = Url::parse(raw).ok()?;
    if !super::release_links::credential_free_https(&url, url.host_str()?) || url.query().is_some() || url.fragment().is_some() { return None; }
    Some(url)
}

fn validate_public_download(download: PublicDownload<'_>) -> Result<(), String> {
    let url = public_https(download.url).ok_or("仅能向外部下载器发送无凭据的公开 HTTPS 附件。")?;
    let page = public_https(download.page_url).ok_or("下载来源页不是公开 HTTPS 地址。")?;
    if download.file_name.is_empty() || download.file_name.len() > 1024 || matches!(download.file_name, "." | "..")
        || download.file_name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        || download.project_name.len() > 4096 || download.tag.len() > 1024
        || [download.project_name, download.tag].iter().any(|s| s.chars().any(char::is_control)) {
        return Err("附件名称或来源信息无效。".into());
    }
    let valid = match url.host_str() {
        Some("github.com") => download.referer.is_none() && page.host_str() == Some("github.com")
            && super::release_links::is_release_link(&url) && network::trusted_download_url(download.url).is_ok(),
        Some("flingtrainer.com") => download.referer == Some(crate::reader::trainer::HOME)
            && crate::reader::catalog::detail_address(download.page_url).is_some()
            && crate::reader::trainer::is_resolved_attachment(&url, download.file_name),
        _ => false,
    };
    if !valid { return Err("只有已核验的公开 GitHub 或风灵月影官网附件可发送。".into()); }
    Ok(())
}

fn local_client(url: &Url) -> Result<reqwest::Client, String> {
    // Do not inherit the OS proxy or DNS mapping for localhost, and never replay or redirect a POST.
    reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never())
        .resolve_to_addrs("localhost", &local_addresses(url))
        .connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(10)).build().map_err(|_| "外部下载器连接初始化失败".into())
}

fn refused(status: StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "下载器拒绝了请求；请检查集成设置。追新不会发送登录凭据或访问密钥。".into(),
        429 => "下载器请求过于频繁，暂时拒绝接收。".into(),
        _ => format!("下载器拒绝了请求（HTTP {}），请检查接口地址和请求模板。", status.as_u16()),
    }
}

fn receipt(body: &[u8], ab: bool) -> Result<ExternalSendOutcome, String> {
    let Ok(text) = std::str::from_utf8(body).map(str::trim) else { return Ok(ExternalSendOutcome::Unknown); };
    if text == "OK" { return Ok(ExternalSendOutcome::Sent); }
    let error_word = |word: &str| matches!(word.to_ascii_lowercase().as_str(), "error" | "failed" | "failure" | "rejected" | "refused");
    let mut accepted = false;
    let mut rejected = error_word(text.split(':').next().unwrap_or_default().trim());
    if let Ok(Value::Object(fields)) = serde_json::from_slice::<Value>(body) {
        for flag in ["ok", "success", "accepted"] {
            accepted |= fields.get(flag).and_then(Value::as_bool) == Some(true);
            rejected |= fields.get(flag).and_then(Value::as_bool) == Some(false);
        }
        if let Some(status) = fields.get("status").and_then(Value::as_str) {
            accepted |= matches!(status.to_ascii_lowercase().as_str(), "ok" | "sent" | "accepted" | "queued" | "success");
            rejected |= error_word(status);
        }
        rejected |= match fields.get("error") {
            None | Some(Value::Null | Value::Bool(false)) => false,
            Some(Value::String(error)) => !error.trim().is_empty(),
            Some(value) => value.as_i64() != Some(0),
        };
    }
    if rejected { return Err("下载器明确拒绝了此任务，请检查接口设置和请求模板。".into()); }
    Ok(if !ab && accepted { ExternalSendOutcome::Sent } else { ExternalSendOutcome::Unknown })
}

async fn send(config: &ExternalDownloader, download: PublicDownload<'_>) -> Result<ExternalSendOutcome, String> {
    validate_public_download(download)?;
    let url = endpoint(&config.endpoint)?;
    let ab = ab_add(config, &url);
    let body = serde_json::to_vec(&request_body(config, download, ab)?).map_err(|_| "无法生成下载器请求。")?;
    if body.len() > MAX_REQUEST_BYTES { return Err("生成的下载器请求过大，请精简接口模板。".into()); }
    // Only metadata and explicit public download headers in JSON; no OAuth, cookies, or local paths.
    let response = match local_client(&url)?.post(url).header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() => return Err("未能连接本机下载器，请打开下载器并启用集成服务。".into()),
        Err(_) => return Ok(ExternalSendOutcome::Unknown),
    };
    let status = response.status();
    if status.is_client_error() && status != StatusCode::REQUEST_TIMEOUT { return Err(refused(status)); }
    if !status.is_success() || response.content_length().is_some_and(|size| size > MAX_RECEIPT_BYTES as u64) {
        return Ok(ExternalSendOutcome::Unknown);
    }
    if !ab && status == StatusCode::NO_CONTENT { return Ok(ExternalSendOutcome::Sent); }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { return Ok(ExternalSendOutcome::Unknown); };
        if chunk.len() > MAX_RECEIPT_BYTES - bytes.len() { return Ok(ExternalSendOutcome::Unknown); }
        bytes.extend_from_slice(&chunk);
    }
    receipt(&bytes, ab)
}
impl Core {
    pub(crate) fn ensure_external_download_enabled(&self) -> Result<(), String> {
        if !self.store.settings()?.use_external_downloader { return Err("尚未启用外部下载器；请在设置中启用，或使用内置下载。".into()); }
        Ok(())
    }

    pub(crate) async fn send_public_download(&self, download: PublicDownload<'_>) -> Result<ExternalSendOutcome, String> {
        let _serial = self.external_requests.try_lock().map_err(|_| "下载器正在处理另一项请求，请稍候。")?;
        let settings = self.store.settings()?;
        if !settings.use_external_downloader { return Err("尚未启用外部下载器；请在设置中启用，或使用内置下载。".into()); }
        send(&settings.external_downloader, download).await
    }

    pub(crate) async fn test_external_downloader(&self, config: ExternalDownloader) -> Result<Value, String> {
        validate_config(&config)?;
        let mut url = endpoint(&config.endpoint)?;
        if ab_add(&config, &url) {
            // GET /queues is read-only, shares /add's optional authentication, and never adds a download.
            url.set_path("/queues");
            let response = match local_client(&url)?.get(url).send().await {
                Ok(response) => response,
                Err(_) => return Ok(json!({"ok":false,"message":"AB 集成接口未连接，请打开下载器并启用浏览器集成服务。"})),
            };
            if matches!(response.status().as_u16(),401|403) {
                return Ok(json!({"ok":false,"message":"AB 接口要求访问密钥，当前配置不支持密钥验证。请核对 AB 的集成设置，或改用内置下载。"}));
            }
            if !response.status().is_success() {
                return Ok(json!({"ok":false,"message":format!("AB 集成接口返回 HTTP {}，请检查端口和接口地址。",response.status().as_u16())}));
            }
            let mut stream=response.bytes_stream();let mut body=Vec::new();
            while let Some(chunk)=stream.next().await {
                let chunk=match chunk {Ok(chunk)=>chunk,Err(_)=>return Ok(json!({"ok":false,"message":"AB 接口响应读取失败，请稍后重试连接测试。"}))};
                if body.len()+chunk.len()>64*1024 {return Ok(json!({"ok":false,"message":"接口响应过大，未确认兼容 AB；请检查端口和接口地址。"}));}
                body.extend_from_slice(&chunk);
            }
            // AB responds with text Content-Type; validate bounded JSON, not its MIME label or a brand claim.
            let compatible=serde_json::from_slice::<Value>(&body).ok().is_some_and(|value|value.as_array().is_some_and(|queues|queues.iter().all(|queue| {
                queue.get("id").is_some_and(|id|id.as_i64().is_some())&&queue.get("name").is_some_and(|name|name.is_string())
            })));
            return Ok(json!({"ok":compatible,"message":if compatible {"AB 兼容集成接口已连接，队列响应正常；测试没有添加下载任务。"}else{"接口响应不符合 AB 队列格式，请检查端口和接口地址。"}}));
        }
        let addresses = local_addresses(&url);
        let reachable = tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(addresses.as_slice())).await;
        let ok = matches!(reachable, Ok(Ok(_)));
        Ok(json!({"ok":ok,"message":if ok { "本机端口可以连接；自定义接口格式会在发送下载时验证。" } else { "本机端口未连接，请打开下载器并启用接口服务。" }}))
    }
    pub(crate) async fn send_external_download(self: &Arc<Self>, project_id: &str, asset_id: u64) -> Result<DownloadRecord, String> {
        let _serial = self.external_requests.lock().await;
        let config = self.store.settings()?.external_downloader; validate_config(&config)?;
        let (record, page) = {
            let _queue = self.download_queue.lock().map_err(|_| "下载队列不可用")?;
            let p = self.store.project(project_id)?; let release = p.release.as_ref().ok_or("请先检查发行版本")?;
            let a = release.assets.iter().find(|a| a.id == asset_id).ok_or("找不到发行附件")?;
            if !a.downloadable || a.size == 0 || a.size > network::MAX_DOWNLOAD_BYTES { return Err("发行附件不可下载或大小超限".into()); }
            network::trusted_download_url(&a.url)?;
            if let Some(existing) = self.store.downloads()?.into_iter().find(|d| d.project_id == p.id && d.asset_id == asset_id && d.tag == release.tag
                && (["queued", "downloading", "installing", "external"].contains(&d.status.as_str()) || (["completed", "installed"].contains(&d.status.as_str()) && local_file_exists(&self.data_dir, d)))) {
                return Ok(existing);
            }
            let time = now(); let d = DownloadRecord { id: uuid::Uuid::new_v4().to_string(), project_id: p.id, project_name: p.name,
                asset_id, file_name: safe_name(&a.name), tag: release.tag.clone(), url: a.url.clone(), kind: a.kind.clone(),
                installable: p.management_mode == "software" && a.installable, total_bytes: a.size, received_bytes: 0,
                status: "external".into(), path: None, sha256: None, verified: false,
                error: Some("正在发送；如程序中断，请先到外部下载器检查任务".into()), created_at: time.clone(), updated_at: time };
            let digest = expected_sha(a.digest.as_deref())?; self.store.set_digest(&d.id, digest.as_deref())?; self.store.put_download(&d)?;
            (d, release.url.clone())
        };
        self.download_event(&record); self.changed();
        self.finish_external_send(&config, &record, &page).await
    }
    async fn finish_external_send(&self, config: &ExternalDownloader, record: &DownloadRecord, page: &str) -> Result<DownloadRecord, String> {
        let result = send(config, PublicDownload {
            url: &record.url, page_url: page, file_name: &record.file_name, project_name: &record.project_name,
            tag: &record.tag, referer: None,
        }).await.and_then(|receipt| match receipt {
            ExternalSendOutcome::Sent => Ok(()),
            ExternalSendOutcome::Unknown => Err(UNKNOWN_RECEIPT.into()),
        });
        let updated = self.store.mutate("downloads", &record.id, |d: &mut DownloadRecord| {
            if d.status == "external" { d.error = result.as_ref().err().cloned(); d.updated_at = now(); }
            Ok(())
        })?;
        self.download_event(&updated); self.changed(); result?; Ok(updated)
    }
    pub(crate) async fn retry_external_download(&self, id: &str) -> Result<DownloadRecord, String> {
        let _serial = self.external_requests.lock().await;
        let record = self.store.download(id)?;
        if record.status != "external" { return Err("只有外部下载任务可重新发送".into()); }
        let config = self.store.settings()?.external_downloader;
        let project = self.store.project(&record.project_id)?;
        let page = format!("{}/releases", project.url);
        self.finish_external_send(&config, &record, &page).await
    }
    pub(crate) async fn import_external_file(&self, id: &str, source: &Path) -> Result<DownloadRecord, String> {
        let _serial = self.external_requests.lock().await;
        let record = self.store.download(id)?;
        if record.status != "external" { return Err("此记录不是待导入的外部下载".into()); }
        let meta = tokio::fs::symlink_metadata(source).await.map_err(|_| "无法读取所选文件")?;
        if !source.is_absolute() || !meta.is_file() || super::download_transfer::linked(&meta) || meta.len() != record.total_bytes { return Err("所选文件不是完整的发行附件，大小不符或是链接".into()); }
        for ancestor in source.ancestors().skip(1) {
            let meta = std::fs::symlink_metadata(ancestor).map_err(|_| "无法读取所选文件目录")?;
            if super::download_transfer::linked(&meta) { return Err("所选文件目录不能使用链接或重解析点".into()); }
        }
        let folder = directory(&self.data_dir, id)?;
        let destination = folder.join(safe_name(&record.file_name));
        let partial = folder.join(format!("external-{}.part", uuid::Uuid::new_v4()));
        let result = async {
            let mut input = tokio::fs::File::open(source).await.map_err(|_| "无法打开所选文件")?;
            let mut out = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&partial).await.map_err(|_| "无法创建导入文件")?;
            let mut hash = Sha256::new(); let mut received = 0u64; let mut buffer = vec![0u8; 256 * 1024];
            loop {
                let n = input.read(&mut buffer).await.map_err(|_| "文件读取失败")?; if n == 0 { break; }
                received += n as u64; if received > record.total_bytes { return Err("文件在导入期间发生变化".into()); }
                hash.update(&buffer[..n]); out.write_all(&buffer[..n]).await.map_err(|_| "文件导入失败，请检查磁盘空间")?;
            }
            if received != record.total_bytes { return Err("文件不完整".into()); }
            out.sync_all().await.map_err(|_| "无法保存导入文件")?; drop(out);
            let digest = format!("{:x}", hash.finalize()); let upstream = self.store.digest(id)?;
            if upstream.as_ref().is_some_and(|expected| expected != &digest) { return Err("SHA-256 校验不符，已拒绝导入".into()); }
            tokio::fs::hard_link(&partial, &destination).await.map_err(|_| "目标文件已存在或无法保存，不会覆盖")?;
            let saved = self.store.mutate("downloads", id, |d: &mut DownloadRecord| {
                if d.status != "external" { return Err("下载状态已经变化".into()); }
                d.status = "completed".into(); d.path = Some(destination.to_string_lossy().into_owned()); d.sha256 = Some(digest);
                d.received_bytes = received; d.verified = upstream.is_some(); d.error = None; d.updated_at = now(); Ok(())
            });
            match saved { Ok(d) => { self.download_event(&d); self.changed(); Ok(d) }, Err(e) => { let _ = tokio::fs::remove_file(&destination).await; Err(e) } }
        }.await;
        let _ = tokio::fs::remove_file(&partial).await; result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn config_is_loopback_and_has_a_real_url_token() {
        let mut config = ExternalDownloader::default(); validate_config(&config).unwrap();
        for url in ["https://example.com:15151/add", "http://127.0.0.1:15151/add?token=secret", "http://127.0.0.1:80/add", "http://user@localhost:15151/add"] {
            config.endpoint = url.into(); assert!(validate_config(&config).is_err());
        }
        config = ExternalDownloader::default(); config.request_template = r#"{"url":"{unknown}"}"#.into(); assert!(validate_config(&config).is_err());
    }
    const TRAINER_FILE: &str = "https://flingtrainer.com/wp-content/uploads/trainer-files/2025/07/Demo.zip/Demo%20Trainer.exe";
    const TRAINER_PAGE: &str = "https://flingtrainer.com/trainer/demo-trainer/";

    fn trainer_fixture() -> PublicDownload<'static> {
        PublicDownload { url: TRAINER_FILE, page_url: TRAINER_PAGE, file_name: "Demo Trainer.exe", project_name: "Demo Trainer", tag: "", referer: Some(crate::reader::trainer::HOME) }
    }

    #[test]
    fn only_resolved_public_attachment_metadata_is_allowed() {
        let trainer = trainer_fixture();
        assert!(validate_public_download(trainer).is_ok());
        for url in [
            "https://flingtrainer.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,",
            "https://flingtrainer.com/download-trainer.php?path=/wp-content/uploads/2025/07/Demo.zip",
            "https://flingtrainer.com/wp-content/uploads/2025/07/Demo.zip?key=value",
            "https://guest@flingtrainer.com/wp-content/uploads/2025/07/Demo.zip",
            "https://elsewhere.test/wp-content/uploads/2025/07/Demo.zip",
        ] { assert!(validate_public_download(PublicDownload { url, ..trainer }).is_err(), "{url}"); }
        assert!(validate_public_download(PublicDownload { file_name: "different.exe", ..trainer }).is_err());
        assert!(validate_public_download(PublicDownload { page_url: crate::reader::trainer::HOME, ..trainer }).is_err());
        assert!(validate_public_download(PublicDownload { referer: None, ..trainer }).is_err());
        assert!(validate_public_download(PublicDownload { referer: Some("https://example.test/"), ..trainer }).is_err());
        for endpoint_text in ["http://@localhost:15151/add", "http:///@localhost:15151/add", "http://127.0.0.1:15151/add#fragment"] {
            assert!(endpoint(endpoint_text).is_err(), "{endpoint_text}");
        }
    }

    #[tokio::test]
    async fn ab_add_and_custom_templates_send_only_verified_fields_and_classify_receipts() {
        use axum::{body::Body, http::{HeaderMap, Response}, routing::post, Router};
        let seen = Arc::new(std::sync::Mutex::new(Vec::<(String, HeaderMap, Value)>::new()));
        let captured = seen.clone();
        let router = Router::new().route("/{target}", post(move |axum::extract::Path(target): axum::extract::Path<String>, headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
            let seen = captured.clone();
            async move {
                seen.lock().unwrap().push((target.clone(), headers, body));
                match target.as_str() {
                    "add" => Response::new(Body::from("OK")),
                    "custom" => Response::new(Body::from(r#"{"accepted":true}"#)),
                    "app-error" => Response::new(Body::from(r#"{"ok":false,"error":"refused"}"#)),
                    "refused" => Response::builder().status(403).body(Body::empty()).unwrap(),
                    "redirect" => Response::builder().status(307).header("Location", "/sink").body(Body::empty()).unwrap(),
                    "unavailable" => Response::builder().status(500).body(Body::empty()).unwrap(),
                    "oversize" => Response::new(Body::from("x".repeat(MAX_RECEIPT_BYTES + 1))),
                    "broken" => Response::new(Body::from_stream(futures_util::stream::iter([
                        Ok(&b"OK"[..]), Err(std::io::Error::other("fixture closed while writing receipt")),
                    ]))),
                    _ => Response::new(Body::from("<html>unrelated service</html>")),
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        let temp = tempfile::tempdir().unwrap();
        let core = Core::new(temp.path().join("data"), true, Arc::new(|_, _| {})).unwrap();
        let mut settings = core.store.settings().unwrap();
        settings.external_downloader.endpoint = format!("http://localhost:{port}/add");
        core.store.save_settings(&settings).unwrap();
        let fixture = trainer_fixture();
        assert!(core.send_public_download(fixture).await.unwrap_err().contains("尚未启用"));
        assert!(seen.lock().unwrap().is_empty());
        settings.use_external_downloader = true;
        core.store.save_settings(&settings).unwrap();
        assert_eq!(core.send_public_download(fixture).await.unwrap(), ExternalSendOutcome::Sent);
        assert!(core.send_public_download(PublicDownload { file_name: "not-real.zip", ..fixture }).await.is_err());
        let mut custom = ExternalDownloader { name: "Local receiver".into(), endpoint: String::new(),
            request_template: r#"{"link":"{url}","page":"{pageUrl}","name":"{fileName}","title":"{projectName}","headers":{"Referer":"{referer}","User-Agent":"{userAgent}"}}"#.into() };
        for (route, outcome) in [("custom", Some(ExternalSendOutcome::Sent)), ("app-error", None), ("refused", None),
            ("redirect", Some(ExternalSendOutcome::Unknown)), ("unavailable", Some(ExternalSendOutcome::Unknown)),
            ("oversize", Some(ExternalSendOutcome::Unknown)), ("broken", Some(ExternalSendOutcome::Unknown)), ("unexpected", Some(ExternalSendOutcome::Unknown))] {
            custom.endpoint = format!("http://127.0.0.1:{port}/{route}");
            let result = send(&custom, fixture).await;
            match outcome {
                Some(expected) => assert_eq!(result.unwrap(), expected, "{route}"),
                None => assert!(result.unwrap_err().contains("拒绝"), "{route}"),
            }
        }
        let captured = seen.lock().unwrap();
        assert_eq!(captured.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
            ["add", "custom", "app-error", "refused", "redirect", "unavailable", "oversize", "broken", "unexpected"]);
        assert_eq!(captured[0].2, json!({"items":[{"type":"http","link":TRAINER_FILE,"downloadPage":TRAINER_PAGE,"suggestedName":fixture.file_name,
            "headers":{"Referer":fixture.referer,"User-Agent":crate::reader::trainer::DOWNLOAD_AGENT}}],"options":{"silentAdd":true,"silentStart":true}}));
        assert_eq!(captured[1].2, json!({"link":TRAINER_FILE,"page":TRAINER_PAGE,"name":fixture.file_name,"title":fixture.project_name,
            "headers":{"Referer":fixture.referer,"User-Agent":crate::reader::trainer::DOWNLOAD_AGENT}}));
        for (_, headers, _) in captured.iter() {
            for name in ["authorization", "cookie", "x-api-key", "proxy-authorization", "referer"] { assert!(!headers.contains_key(name), "{name}"); }
        }
        assert_eq!(serde_json::to_value(ExternalSendOutcome::Sent).unwrap(), json!({"status":"sent"}));
        assert_eq!(serde_json::to_value(ExternalSendOutcome::Unknown).unwrap(), json!({"status":"unknown"}));
        drop(captured);
        assert!(core.store.downloads().unwrap().is_empty());
        server.abort();
    }

    #[test]
    fn receipt_distinguishes_protocol_acknowledgement_from_ambiguous_or_negative_data() {
        assert_eq!(receipt(b" OK\r\n", true).unwrap(), ExternalSendOutcome::Sent);
        assert_eq!(receipt(br#"{"success":true}"#, true).unwrap(), ExternalSendOutcome::Unknown);
        assert_eq!(receipt(br#"{"status":"queued","error":null}"#, false).unwrap(), ExternalSendOutcome::Sent);
        for raw in ["", "<html>OK</html>", "{}", "true"] { assert_eq!(receipt(raw.as_bytes(), true).unwrap(), ExternalSendOutcome::Unknown); }
        for raw in ["Error: invalid input", r#"{"accepted":false}"#, r#"{"success":true,"error":{"message":"refused"}}"#] {
            assert!(receipt(raw.as_bytes(), true).is_err(), "{raw}");
        }
    }

    #[tokio::test] async fn ab_connection_test_checks_read_only_bounded_queues_without_credentials() {
        use axum::{http::StatusCode, routing::{get, post}, Router};
        let temp=tempfile::tempdir().unwrap();let core=Core::new(temp.path().join("data"),true,Arc::new(|_,_|{})).unwrap();
        for (status,body,ok,reason) in [
            (StatusCode::OK,r#"[{"id":1,"name":"main"}]"#.to_owned(),true,"队列响应正常"),
            (StatusCode::OK,"[]".to_owned(),true,"队列响应正常"),
            (StatusCode::OK,r#"[{"id":"1","name":"main"}]"#.to_owned(),false,"队列格式"),
            (StatusCode::OK,r#"{"id":1,"name":"main"}"#.to_owned(),false,"队列格式"),
            (StatusCode::OK,"<html>other service</html>".to_owned(),false,"队列格式"),
            (StatusCode::OK,"x".repeat(64*1024+1),false,"响应过大"),
            (StatusCode::UNAUTHORIZED,"denied".to_owned(),false,"访问密钥"),
            (StatusCode::FORBIDDEN,"denied".to_owned(),false,"访问密钥"),
            (StatusCode::FOUND,"redirect".to_owned(),false,"HTTP 302"),
            (StatusCode::INTERNAL_SERVER_ERROR,"failed".to_owned(),false,"HTTP 500"),
        ] {
            let reads=Arc::new(std::sync::Mutex::new(Vec::new()));let seen=reads.clone();
            let adds=Arc::new(std::sync::atomic::AtomicUsize::new(0));let posted=adds.clone();
            let router=Router::new().route("/queues",get(move |headers:axum::http::HeaderMap| {
                let seen=seen.clone();let body=body.clone();async move {seen.lock().unwrap().push(headers);(status,[("content-type","text/plain")],body)}
            })).route("/add",post(move || {let posted=posted.clone();async move {posted.fetch_add(1,std::sync::atomic::Ordering::SeqCst);"OK"}}));
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();
            let server=tokio::spawn(async move {axum::serve(listener,router).await.unwrap();});
            let mut config=ExternalDownloader::default();config.endpoint=format!("http://127.0.0.1:{port}/add");
            let result=core.test_external_downloader(config).await.unwrap();
            assert_eq!(result["ok"],ok,"{reason}");assert!(result["message"].as_str().unwrap().contains(reason),"{result}");
            assert_eq!(adds.load(std::sync::atomic::Ordering::SeqCst),0);
            let captured=reads.lock().unwrap();assert_eq!(captured.len(),1);
            for secret in ["authorization","cookie","x-api-key"] {assert!(!captured[0].contains_key(secret));}
            drop(captured);assert!(core.store.downloads().unwrap().is_empty());server.abort();
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();drop(listener);
        let mut config=ExternalDownloader::default();config.endpoint=format!("http://127.0.0.1:{port}/add");
        let result=core.test_external_downloader(config).await.unwrap();assert_eq!(result["ok"],false);assert!(result["message"].as_str().unwrap().contains("未连接"));
    }
    fn fixture(core: &Core, content: &[u8]) -> (Project, DownloadRecord) {
        let mut p = test_project("example", "app"); let time = now();
        let d = DownloadRecord { id: uuid::Uuid::new_v4().to_string(), project_id: p.id.clone(), project_name: p.name.clone(), asset_id: 1,
            file_name: "fixture.zip".into(), tag: "v1".into(), url: "https://github.com/example/app/releases/download/v1/fixture.zip".into(), kind: "portable".into(),
            installable: true, total_bytes: content.len() as u64, received_bytes: 0, status: "external".into(), path: None, sha256: None, verified: false, error: None, created_at: time.clone(), updated_at: time };
        p.release = Some(Release { id: 1, tag: "v1".into(), name: "v1".into(), body: String::new(), url: "https://github.com/example/app/releases/tag/v1".into(), published_at: now(), prerelease: false,
            assets: vec![ReleaseAsset { id: 1, name: d.file_name.clone(), url: d.url.clone(), size: d.total_bytes, content_type: "application/zip".into(), digest: Some(format!("sha256:{:x}", Sha256::digest(content))), kind: "portable".into(), compatible: true, downloadable: true, installable: true, reason: String::new(), score: 1 }] });
        core.store.put_project(&p).unwrap(); (p, d)
    }
    #[tokio::test] async fn external_send_is_confirmed_deduplicated_and_preserves_uncertain_receipts() {
        use axum::{routing::post, Router};
        let seen = Arc::new(std::sync::Mutex::new(Vec::<(axum::http::HeaderMap, Value)>::new()));
        let received = seen.clone();
        let router = Router::new().route("/send", post(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
            let received = received.clone(); async move { received.lock().unwrap().push((headers, body)); "OK" }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        let temp = tempfile::tempdir().unwrap(); let core = Core::new(temp.path().join("data"), true, Arc::new(|_,_|{})).unwrap();
        let (p, _) = fixture(&core, b"fixture bytes"); let mut settings = core.store.settings().unwrap(); settings.external_downloader.endpoint = format!("http://127.0.0.1:{port}/send"); core.store.save_settings(&settings).unwrap();
        assert!(core.handle("send_external_download", json!({"projectId":p.id,"assetId":1})).await.is_err()); assert!(seen.lock().unwrap().is_empty());
        let first = core.send_external_download(&p.id, 1).await.unwrap(); let second = core.send_external_download(&p.id, 1).await.unwrap(); assert_eq!(first.id, second.id);
        let captured = seen.lock().unwrap(); assert_eq!(captured.len(), 1); assert!(!captured[0].0.contains_key("authorization")); assert!(!captured[0].0.contains_key("cookie")); assert_eq!(captured[0].1["items"][0]["link"], first.url); assert_eq!(captured[0].1["items"][0]["suggestedName"], first.file_name); assert_eq!(captured[0].1["options"]["silentAdd"], true); assert_eq!(captured[0].1["options"]["silentStart"], true); drop(captured);
        server.abort(); assert!(core.retry_external_download(&first.id).await.is_err()); let retained = core.store.download(&first.id).unwrap(); assert_eq!(retained.status, "external"); assert!(retained.error.is_some());
    }
    #[tokio::test] async fn external_import_checks_digest_and_never_overwrites() {
        let temp = tempfile::tempdir().unwrap(); let core = Core::new(temp.path().join("data"), true, Arc::new(|_,_|{})).unwrap();
        let content = b"fixture bytes"; let (_, d) = fixture(&core, content); core.store.put_download(&d).unwrap(); core.store.set_digest(&d.id, Some(&format!("{:x}", Sha256::digest(content)))).unwrap();
        let source = temp.path().join("fixture.zip"); tokio::fs::write(&source, b"wrongxx bytes").await.unwrap();
        assert!(core.import_external_file(&d.id, &source).await.is_err()); assert_eq!(core.store.download(&d.id).unwrap().status, "external");
        tokio::fs::write(&source, content).await.unwrap(); let folder = directory(&core.data_dir, &d.id).unwrap(); let destination = folder.join(&d.file_name); tokio::fs::write(&destination, b"keep me").await.unwrap();
        assert!(core.import_external_file(&d.id, &source).await.is_err()); assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"keep me"); tokio::fs::remove_file(&destination).await.unwrap();
        let imported = core.import_external_file(&d.id, &source).await.unwrap(); assert_eq!(imported.status, "completed"); assert!(imported.verified); assert!(local_file_exists(&core.data_dir, &imported)); assert_eq!(tokio::fs::read(&source).await.unwrap(), content);
        assert!(core.import_external_file(&d.id, &source).await.is_err()); assert_eq!(std::fs::read_dir(folder).unwrap().count(), 1);
    }
    #[test] fn template_substitution_preserves_json_and_never_executes_data() {
        let record: DownloadRecord = serde_json::from_value(json!({"id":"fixture","projectId":"fixture","projectName":"test \" {url}","assetId":1,"fileName":"file.zip","tag":"v1","url":"https://github.com/example/app/releases/download/v1/file.zip","kind":"portable","totalBytes":3,"receivedBytes":0,"status":"external","path":null,"sha256":null,"verified":false,"error":null,"createdAt":now(),"updatedAt":now()})).unwrap();
        let mut config = ExternalDownloader::default(); config.request_template = r#"{"url":"{url}","name":"{projectName}"}"#.into();
        let body = request_body(&config, PublicDownload { url: &record.url, page_url: "https://github.com/example/app/releases",
            file_name: &record.file_name, project_name: &record.project_name, tag: &record.tag, referer: None }, false).unwrap();
        assert_eq!(body["name"], record.project_name); assert_eq!(body["url"], record.url);
    }
}
