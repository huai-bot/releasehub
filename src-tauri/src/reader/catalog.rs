//! Read-only catalog data. Remote markup stays here; the main window receives text and vetted URLs.
use super::trainer;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use dom_query::Document;
use reqwest::{header::{self, HeaderMap}, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{collections::{HashSet, VecDeque}, ffi::OsStr, path::{Path, PathBuf}, sync::Mutex, time::{Duration, Instant}};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Semaphore;
use url::Url;

use crate::{core::{Core, ExternalSendOutcome, PublicDownload}, proxy::ProxyState};

const API_PATH: &str = "/wp-json/wp/v2/posts";
const PAGE_SIZE: usize = 24;
const MAX_PAGE: u32 = 10_000;
const MAX_QUERY_BYTES: usize = 512;
const MAX_LIST_BYTES: usize = 512 * 1024;
const MAX_DETAIL_BYTES: usize = 2 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 128;
const DETAIL_LINK_ERROR: &str = "只能打开风灵月影官网的修改器详情。";
const FORMAT_ERROR: &str = "未能识别风灵月影官网的内容，请稍后重试或用浏览器打开官网。";
const TOO_LARGE: &str = "官网返回的内容过大，请用浏览器打开官网。";
const INACTIVE: &str = "script, style, noscript, template, iframe, object, embed, svg, math, form, [hidden], [aria-hidden='true'], .fling-entity-placement";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode { Latest, All, Search }

#[derive(Deserialize)]
struct ReadInput<T> {
    #[serde(flatten)]
    input: T,
    #[serde(default, rename = "forceRefresh")]
    force_refresh: bool,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct ListInput { mode: Mode, query: String, page: u32 }

#[derive(Deserialize)]
struct DetailInput { url: String }

impl ListInput {
    fn validate(mut self) -> Result<Self, String> {
        if !(1..=MAX_PAGE).contains(&self.page) { return Err("官网目录页码无效。".into()); }
        if self.mode == Mode::Search {
            if self.query.len() > MAX_QUERY_BYTES || self.query.chars().any(disallowed) {
                return Err("搜索词过长或含无效字符，请输入简短的游戏名称。".into());
            }
            let word = self.query.trim();
            if word.is_empty() { return Err("请输入要查找的游戏名称。".into()); }
            self.query = word.to_owned();
        } else {
            self.query.clear();
        }
        Ok(self)
    }

    fn url(&self) -> Url {
        let mut url = homepage();
        url.set_path(API_PATH);
        let (orderby, order) = match self.mode {
            Mode::Latest => ("modified", "desc"),
            Mode::All => ("title", "asc"),
            Mode::Search => ("relevance", "desc"),
        };
        let mut query = url.query_pairs_mut();
        // Category 5 is FLiNG's public "Trainer" category, excluding unrelated site posts.
        query.append_pair("categories", "5").append_pair("per_page", &PAGE_SIZE.to_string())
            .append_pair("page", &self.page.to_string()).append_pair("orderby", orderby)
            .append_pair("order", order).append_pair("_fields", "link,title,excerpt,date_gmt,modified_gmt");
        if self.mode == Mode::Search { query.append_pair("search", &self.query); }
        drop(query);
        url
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrainerEntry {
    title: String, url: String, published_at: Option<String>, updated_at: Option<String>, summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrainerListPage {
    items: Vec<TrainerEntry>, page: u32, has_next_page: bool, total_pages: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct TrainerAttachment { label: String, url: String }

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TrainerDetail {
    #[serde(flatten)]
    entry: TrainerEntry,
    content: String,
    attachments: Vec<TrainerAttachment>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub(crate) enum TrainerSaveResult { Saved { path: PathBuf }, Cancelled }

struct Cache<K, V> {
    entries: Mutex<VecDeque<(K, Instant, V)>>,
    ttl: Duration,
    capacity: usize,
}

impl<K: PartialEq, V: Clone> Cache<K, V> {
    fn new(seconds: u64, capacity: usize) -> Self {
        Self { entries: Mutex::new(VecDeque::new()), ttl: Duration::from_secs(seconds), capacity }
    }

    fn get(&self, key: &K, force_refresh: bool) -> Option<V> {
        if force_refresh { return None; }
        let mut entries = self.entries.lock().ok()?;
        entries.retain(|(_, created, _)| created.elapsed() < self.ttl);
        let index = entries.iter().position(|(cached, _, _)| cached == key)?;
        let entry = entries.remove(index)?;
        let value = entry.2.clone();
        entries.push_back(entry);
        Some(value)
    }

    fn put(&self, key: K, value: V) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(cached, created, _)| *cached != key && created.elapsed() < self.ttl);
            if entries.len() >= self.capacity { entries.pop_front(); }
            entries.push_back((key, Instant::now(), value));
        }
    }
}

pub(crate) struct Catalog {
    pages: Cache<ListInput, TrainerListPage>,
    details: Cache<Url, TrainerDetail>,
    reads: Semaphore,
    saving: tokio::sync::Mutex<()>,
    last_saved: Mutex<Option<PathBuf>>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self { pages: Cache::new(90, 32), details: Cache::new(180, 16), reads: Semaphore::new(4),
            saving: tokio::sync::Mutex::new(()), last_saved: Mutex::new(None) }
    }
}

impl Catalog {
    pub(crate) async fn list(&self, proxy: &ProxyState, input: serde_json::Value) -> Result<TrainerListPage, String> {
        let ReadInput::<ListInput> { input, force_refresh } = serde_json::from_value(input).map_err(|_| "官网目录查询参数无效。")?;
        let input = input.validate()?;
        if let Some(page) = self.pages.get(&input, force_refresh) { return Ok(page); }
        let _slot = self.reads.try_acquire().map_err(|_| "正在查询官网，请稍后重试。")?;
        let response = request(proxy, &input.url(), "application/json", force_refresh).await?;
        let status = response.status();
        if status != StatusCode::OK && status != StatusCode::BAD_REQUEST { return Err(status_error(status)); }
        let headers = pagination_header(response.headers());
        let bytes = response_body(response, "application/json", MAX_LIST_BYTES).await?;
        if status == StatusCode::BAD_REQUEST {
            if serde_json::from_slice::<serde_json::Value>(&bytes).ok()
                .is_some_and(|value| value["code"].as_str() == Some("rest_post_invalid_page_number")) {
                return Err("这一页已超出官网目录范围，请返回上一页。".into());
            }
            return Err(status_error(status));
        }
        let page = input.page;
        let result = tokio::task::spawn_blocking(move || parse_list(&bytes, page, headers)).await.map_err(|_| FORMAT_ERROR)??;
        self.pages.put(input, result.clone());
        Ok(result)
    }

    async fn fetch_detail(&self, proxy: &ProxyState, url: Url, force_refresh: bool) -> Result<TrainerDetail, String> {
        let _slot = self.reads.try_acquire().map_err(|_| "正在查询官网，请稍后重试。")?;
        let response = request(proxy, &url, "text/html", force_refresh).await?;
        if response.status() != StatusCode::OK { return Err(status_error(response.status())); }
        let bytes = response_body(response, "text/html", MAX_DETAIL_BYTES).await?;
        tokio::task::spawn_blocking(move || {
            let html = std::str::from_utf8(&bytes).map_err(|_| FORMAT_ERROR)?;
            parse_detail(html, &url)
        }).await.map_err(|_| FORMAT_ERROR)?
    }

    pub(crate) async fn detail(&self, proxy: &ProxyState, input: serde_json::Value) -> Result<TrainerDetail, String> {
        let ReadInput::<DetailInput> { input, force_refresh } = serde_json::from_value(input).map_err(|_| DETAIL_LINK_ERROR)?;
        let url = detail_address(&input.url).ok_or(DETAIL_LINK_ERROR)?;
        if let Some(detail) = self.details.get(&url, force_refresh) { return Ok(detail); }
        let detail = self.fetch_detail(proxy, url.clone(), force_refresh).await?;
        self.details.put(url, detail.clone());
        Ok(detail)
    }

    async fn verified_attachment(&self, proxy: &ProxyState, detail_url: &str, attachment_url: &str) -> Result<(TrainerEntry, Url), String> {
        let detail_url = detail_address(detail_url).ok_or(DETAIL_LINK_ERROR)?;
        let attachment = trainer::parse_url(attachment_url).filter(trainer::is_attachment).ok_or(trainer::BAD_LINK)?;
        // Re-read the article so a caller cannot offer an arbitrary or previously removed attachment.
        let detail = self.fetch_detail(proxy, detail_url.clone(), true).await?;
        let entry = contains_attachment(&detail, &attachment).then(|| detail.entry.clone());
        self.details.put(detail_url, detail);
        Ok((entry.ok_or("该详情没有列出这个附件，请刷新详情后重试。")?, attachment))
    }

    pub(crate) async fn send_external(&self, core: &Core, detail_url: &str, attachment_url: &str) -> Result<ExternalSendOutcome, String> {
        core.ensure_external_download_enabled()?;
        let _saving = self.saving.try_lock().map_err(|_| "已有一个官网附件正在处理。")?;
        let (article, attachment) = self.verified_attachment(&core.proxy, detail_url, attachment_url).await?;
        let (url, name) = trainer::resolve_for_external(&core.proxy, attachment).await?;
        core.send_public_download(PublicDownload {
            url: url.as_str(), page_url: &article.url, file_name: &name, project_name: &article.title,
            tag: "", referer: Some(trainer::HOME),
        }).await
    }

    pub(crate) async fn save(&self, app: &AppHandle, proxy: &ProxyState, detail_url: &str, attachment_url: &str) -> Result<TrainerSaveResult, String> {
        let _saving = self.saving.try_lock().map_err(|_| "已有一个官网附件正在处理。")?;
        let (_, attachment) = self.verified_attachment(proxy, detail_url, attachment_url).await?;
        let Some(path) = trainer::save_from_catalog(app, proxy, attachment).await? else {
            return Ok(TrainerSaveResult::Cancelled);
        };
        *self.last_saved.lock().map_err(|_| "无法记录本次另存的附件。")? = Some(path.clone());
        Ok(TrainerSaveResult::Saved { path })
    }

    pub(crate) fn reveal(&self, app: &AppHandle, path: &str) -> Result<(), String> {
        let saved = self.last_saved.lock().map_err(|_| "无法查看本次保存记录。")?;
        let path = matching_saved_path(saved.as_deref(), path).ok_or("只能显示这次刚保存的官网附件。")?;
        trainer::reveal(app, path)
    }
}

fn matching_saved_path<'a>(saved: Option<&'a Path>, requested: &str) -> Option<&'a Path> {
    saved.filter(|path| path.as_os_str() == OsStr::new(requested))
}

fn homepage() -> Url { Url::parse(trainer::HOME).expect("static homepage") }

pub(crate) fn detail_address(raw: &str) -> Option<Url> {
    let slug = raw.strip_prefix(trainer::HOME)?.strip_prefix("trainer/")?;
    let slug = slug.strip_suffix('/').unwrap_or(slug);
    if !trainer::segment(slug) { return None; }
    let mut url = trainer::parse_url(raw)?;
    if url.query().is_some() || url.fragment().is_some() { return None; }
    // Canonical URL with its trailing slash, without relying on site redirects.
    if !raw.ends_with('/') { url.set_path(&format!("{}/", url.path())); }
    Some(url)
}

pub(crate) fn open_original(app: &AppHandle, url: &str) -> Result<(), String> {
    let url = if url == trainer::HOME { homepage() } else { detail_address(url).ok_or(DETAIL_LINK_ERROR)? };
    app.opener().open_url(url.as_str(), None::<&str>).map_err(|_| "无法打开系统浏览器。".into())
}

async fn request(proxy: &ProxyState, url: &Url, accept: &str, force_refresh: bool) -> Result<Response, String> {
    let client = proxy.client_builder()?.https_only(true).redirect(reqwest::redirect::Policy::none()).referer(false)
        .user_agent(trainer::DOWNLOAD_AGENT).connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(15))
        .build().map_err(|_| "无法创建风灵月影官网连接。")?;
    let mut request = client.get(url.clone()).header(header::ACCEPT, accept).header(header::ACCEPT_ENCODING, "identity");
    if force_refresh { request = request.header(header::CACHE_CONTROL, "no-cache"); }
    request.send().await.map_err(|error| if error.is_timeout() {
            "连接风灵月影官网超时，请检查网络或代理后重试。".into()
        } else { "暂时无法连接风灵月影官网，请检查网络或代理后重试。".into() })
}

fn status_error(status: StatusCode) -> String {
    match status {
        StatusCode::NOT_FOUND => "风灵月影官网没有找到该内容，条目可能已移除。".into(),
        StatusCode::TOO_MANY_REQUESTS => "官网暂时限制查询频率，请稍后重试。".into(),
        StatusCode::PROXY_AUTHENTICATION_REQUIRED => "代理验证未通过，请检查追新的网络代理设置。".into(),
        status if status.is_redirection() => "官网地址发生跳转，请用浏览器打开官网查看。".into(),
        _ => format!("风灵月影官网暂时不可用（HTTP {}），请稍后重试。", status.as_u16()),
    }
}

fn one_header(headers: &HeaderMap, name: header::HeaderName) -> Result<Option<&str>, String> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().map(|value| value.to_str().map_err(|_| FORMAT_ERROR)).transpose()?;
    if values.next().is_some() { return Err(FORMAT_ERROR.into()); }
    Ok(value)
}

async fn response_body(mut response: Response, media_type: &str, limit: usize) -> Result<Vec<u8>, String> {
    let headers = response.headers();
    let content_type = one_header(headers, header::CONTENT_TYPE)?.ok_or(FORMAT_ERROR)?;
    if !content_type.split(';').next().unwrap_or_default().trim().eq_ignore_ascii_case(media_type)
        || one_header(headers, header::CONTENT_ENCODING)?.is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
        || headers.contains_key(header::CONTENT_RANGE) { return Err(FORMAT_ERROR.into()); }
    if response.content_length().is_some_and(|length| length > limit as u64) { return Err(TOO_LARGE.into()); }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取官网时连接中断，请重试。")? {
        if chunk.len() > limit - body.len() { return Err(TOO_LARGE.into()); }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn pagination_header(headers: &HeaderMap) -> Option<u32> {
    one_header(headers, header::HeaderName::from_static("x-wp-totalpages")).ok().flatten()?
        .parse::<u32>().ok().filter(|total| *total <= MAX_PAGE)
}

#[derive(Deserialize)]
struct WpPost { link: String, title: Rendered, excerpt: Option<Rendered>, date_gmt: Option<String>, modified_gmt: Option<String> }

#[derive(Deserialize)]
struct Rendered { rendered: String, #[serde(default)] protected: bool }

fn parse_list(bytes: &[u8], page: u32, total_pages: Option<u32>) -> Result<TrainerListPage, String> {
    let posts: Vec<WpPost> = serde_json::from_slice(bytes).map_err(|_| FORMAT_ERROR)?;
    if posts.len() > PAGE_SIZE { return Err(FORMAT_ERROR.into()); }
    let count = posts.len();
    let mut seen = HashSet::new();
    let items: Vec<_> = posts.into_iter().filter_map(|post| {
        let url = detail_address(&post.link)?.into();
        let title = fragment_text(&post.title.rendered, 200);
        if title.is_empty() || post.title.protected || !seen.insert(String::clone(&url)) { return None; }
        let summary = post.excerpt.filter(|excerpt| !excerpt.protected)
            .map_or_else(String::new, |excerpt| fragment_text(&excerpt.rendered, 280));
        Some(TrainerEntry { title, url, summary, published_at: date(post.date_gmt.as_deref()), updated_at: date(post.modified_gmt.as_deref()) })
    }).collect();
    if count != 0 && items.is_empty() { return Err(FORMAT_ERROR.into()); }
    let total_pages = total_pages.filter(|total| if count == 0 { *total == 0 || *total < page } else { *total >= page });
    let has_next_page = page < MAX_PAGE && total_pages.map_or(count == PAGE_SIZE, |total| page < total);
    Ok(TrainerListPage { items, page, has_next_page, total_pages })
}

fn date(raw: Option<&str>) -> Option<String> {
    let raw = raw.filter(|value| value.len() <= 64 && !value.starts_with("0000-"))?;
    DateTime::parse_from_rfc3339(raw).map(|time| time.with_timezone(&Utc)).ok()
        .or_else(|| NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S").ok().map(|time| time.and_utc()))
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn disallowed(ch: char) -> bool {
    ch.is_control() || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn clean_text(raw: &str, limit: usize, keep_lines: bool) -> String {
    let filtered: String = raw.chars().filter(|ch| !disallowed(*ch) || ch.is_whitespace()).collect();
    let text = if keep_lines {
        let mut result = String::new();
        let mut blank = false;
        for line in filtered.lines() {
            let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if line.is_empty() { blank = !result.is_empty(); continue; }
            if !result.is_empty() { result.push_str(if blank { "\n\n" } else { "\n" }); }
            result.push_str(&line);
            blank = false;
        }
        result
    } else { filtered.split_whitespace().collect::<Vec<_>>().join(" ") };
    text.chars().take(limit).collect::<String>().trim_end().to_owned()
}

fn fragment_text(html: &str, limit: usize) -> String {
    if html.is_empty() { return String::new(); }
    let doc = Document::fragment(html);
    doc.select(INACTIVE).remove();
    clean_text(&doc.formatted_text(), limit, false)
}

fn contains_attachment(detail: &TrainerDetail, url: &Url) -> bool {
    detail.attachments.iter().any(|item| item.url == url.as_str())
}

fn parse_detail(html: &str, url: &Url) -> Result<TrainerDetail, String> {
    let doc = Document::from(html);
    if let Some(canonical) = doc.select_single("head link[rel='canonical']").attr("href") {
        if detail_address(canonical.as_ref()).as_ref() != Some(url) { return Err(FORMAT_ERROR.into()); }
    }
    doc.select(INACTIVE).remove();
    let article = doc.select_single("article.category-trainer");
    let title = clean_text(&article.select_single("h1.post-title").formatted_text(), 200, false);
    let entry = article.select_single(".entry");
    if title.is_empty() || !entry.exists() { return Err(FORMAT_ERROR.into()); }

    let mut seen = HashSet::new();
    let attachments = entry.select(".download-attachments a[href]").iter().filter_map(|link| {
        let href = link.attr("href")?;
        let href = href.trim();
        if href.len() > 4096 || href.chars().any(|ch| ch == '\\' || disallowed(ch) || ch == '\u{fffd}') { return None; }
        let target = url.join(href).ok().filter(trainer::is_attachment)?;
        let addr = target.to_string();
        if !seen.insert(addr.clone()) { return None; }
        let label = clean_text(&link.formatted_text(), 240, false);
        let label = if label.is_empty() { "官网附件".into() } else { label };
        Some(TrainerAttachment { label, url: addr })
    }).take(MAX_ATTACHMENTS).collect();
    entry.select(".download-attachments").remove();

    let content = clean_text(&entry.formatted_text(), 12_000, true);
    let summary = clean_text(content.lines().next().unwrap_or_default(), 280, false);
    let published = doc.select_single("head meta[property='article:published_time']").attr("content");
    let modified = doc.select_single("head meta[property='article:modified_time']").attr("content");
    Ok(TrainerDetail {
        entry: TrainerEntry { title, url: url.to_string(), published_at: date(published.as_deref()), updated_at: date(modified.as_deref()), summary },
        content,
        attachments,
    })
}

#[cfg(test)]
mod tests;
