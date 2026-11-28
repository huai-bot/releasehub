pub(crate) mod trainer;

use crate::core::release_links::{credential_free_https, is_release_link};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{webview::{Color, DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder}, AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Rect, Theme, WebviewUrl};
use tauri_plugin_opener::OpenerExt;
use url::Url;
use uuid::Uuid;

const PREFIX: &str = "github-reader-";
const STYLE: &str = concat!(include_str!("../../src/soft-material.css"), "\n", include_str!("reader.css"));
const APPLY_THEME: &str = include_str!("reader-theme.js");

#[derive(Deserialize)]
struct Bounds { x: f64, y: f64, width: f64, height: f64 }

impl Bounds {
    fn rect(&self) -> Result<Rect,String> {
        if ![self.x, self.y, self.width, self.height].iter().all(|value| (0.0..=32768.0).contains(value)) || self.width < 1.0 || self.height < 1.0 {
            return Err("阅读区域尚未准备好。".into());
        }
        Ok(Rect { position: LogicalPosition::new(self.x,self.y).into(), size: LogicalSize::new(self.width,self.height).into() })
    }
}

fn website(url: &Url) -> bool {
    credential_free_https(url, "github.com")
}

fn readable(url: &Url) -> bool {
    if !website(url) { return false; }
    let first = url.path().split('/').nth(1).unwrap_or("");
    !matches!(first, "login" | "logout" | "session" | "sessions" | "settings" | "signup" | "account" | "password_reset") &&
        !(first == "orgs" && url.path().split('/').nth(3) == Some("sso"))
}

fn release_asset(url: &Url) -> bool {
    is_release_link(url)
}

fn label(payload: &Value) -> Result<String,String> {
    let id = payload["id"].as_str().and_then(|text| Uuid::parse_str(text).ok()).ok_or("阅读页编号无效。")?;
    Ok(format!("{PREFIX}{id}"))
}

fn bounds(payload: &Value) -> Result<Rect,String> {
    serde_json::from_value::<Bounds>(payload["bounds"].clone()).map_err(|_| "阅读区域无效。".to_string())?.rect()
}

fn send(app: &AppHandle, label: &str, phase: &str, value: &str) {
    let _ = app.emit_to(EventTarget::webview("main"), "releasehub:reader", json!({"id":&label[PREFIX.len()..],"phase":phase,"value":value}));
}

pub(crate) fn close_active(app: &AppHandle) -> Result<(),String> {
    for (label,view) in app.webviews() {
        if label.starts_with(PREFIX) {
            view.close().map_err(|_| "无法关闭原文，请返回后重试。")?;
            send(app,&label,"closed","");
        }
    }
    Ok(())
}

pub(crate) fn open(app: &AppHandle, input: Value, profile: PathBuf, mode: &str, proxy: Option<Url>) -> Result<Value,String> {
    let label = label(&input)?;
    let url = input["url"].as_str().and_then(|text| Url::parse(text).ok()).filter(readable).ok_or("这里只能打开 GitHub 内容页。")?;
    let rect = bounds(&input)?;
    let window = app.get_window("main").ok_or("主窗口未打开。")?;
    let theme = match input["theme"].as_str() { Some("light") => "light", Some("dark") => "dark", _ => "system" };
    let dark = theme == "dark" || (theme == "system" && window.theme().ok() == Some(Theme::Dark));
    let background = if dark { Color(36, 33, 30, 255) } else { Color(242, 232, 224, 255) };
    close_active(app)?;
    let nav_app = app.clone(); let nav_label = label.clone();
    let popup_app = app.clone(); let popup_label = label.clone();
    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        .data_directory(profile).disable_drag_drop_handler()
        .background_color(background)
        .initialization_script(format!("({APPLY_THEME})({});", json!({"theme":theme,"css":STYLE})))
        .on_navigation(move |url| {
            if release_asset(url) { send(&nav_app,&nav_label,"download",url.as_str()); false }
            else if readable(url) { true } else {
                send(&nav_app, &nav_label, "blocked", if website(url) {url.as_str()} else {""}); false
            }
        })
        .on_new_window(move |url, _| {
            if release_asset(&url) { send(&popup_app,&popup_label,"download",url.as_str()); }
            else if readable(&url) {
                if let Some(view) = popup_app.get_webview(&popup_label) { tauri::async_runtime::spawn(async move { let _ = view.navigate(url); }); }
            } else { send(&popup_app,&popup_label,"blocked",if website(&url) {url.as_str()} else {""}); }
            NewWindowResponse::Deny
        })
        .on_download(|view,event| {
            if let DownloadEvent::Requested { url, .. } = event {
                send(view.app_handle(),view.label(),"download",if release_asset(&url) { url.as_str() } else { "" });
            }
            false
        })
        .on_document_title_changed(|view,title| { send(view.app_handle(),view.label(),"title",&title.chars().take(160).collect::<String>()); })
        .on_page_load(|view,event| { if readable(event.url()) { send(view.app_handle(),view.label(),if event.event()==PageLoadEvent::Finished {"ready"} else {"loading"},event.url().as_str()); } });
    if let Some(proxy) = proxy { builder = builder.proxy_url(proxy); }
    else if mode == "direct" { builder = builder.additional_browser_args("--no-proxy-server"); }
    window.add_child(builder,rect.position,rect.size).map_err(|_| "网页未能打开，请重试或使用浏览器。")?;
    Ok(Value::Null)
}

pub(crate) fn control(app: &AppHandle, action: &str, input: Value) -> Result<Value,String> {
    let label = label(&input)?;
    let Some(view) = app.get_webview(&label) else { return if action == "reader_external" {Err("阅读页已关闭。".into())} else {Ok(Value::Null)}; };
    match action {
        "reader_resize" => view.set_bounds(bounds(&input)?).map_err(|_| "无法调整阅读区域。")?,
        "reader_close" => view.close().map_err(|_| "无法关闭阅读页。")?,
        "reader_external" => {
            let url = view.url().ok().filter(website).ok_or("当前网页地址不可用。")?;
            app.opener().open_url(url.as_str(),None::<&str>).map_err(|_| "无法打开系统浏览器。")?;
        }
        _ => return Err("未知阅读操作。".into()),
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_limits_navigation_to_content_and_downloads_to_explicit_release_assets() {
        let parse = |value: &str| Url::parse(value).unwrap();
        assert!(readable(&parse("https://github.com/sample/tool/discussions/3")));
        assert!(release_asset(&parse("https://github.com/sample/tool/releases/download/v2/file.zip")));
        assert!(!release_asset(&parse("https://github.com/sample/tool/releases/tag/v2")));
        for address in ["https://github.com/login/device", "https://github.com/settings/profile", "https://github.com/orgs/example/sso", "https://example.org/sample/tool", "http://github.com/sample/tool"] {
            assert!(!readable(&parse(address)));
        }
        assert!(label(&json!({"id":"main"})).is_err());
        assert!(bounds(&json!({"bounds":{"x":-48,"y":50,"width":100,"height":90}})).is_err());
    }

}
