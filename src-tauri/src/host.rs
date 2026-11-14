use crate::{agent::{self, RunningAgent}, core::Core, secrets::{random_token, SecretStore}};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::{Duration, Instant}};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

struct HostState {
    core: Arc<Core>,
    data_dir: PathBuf,
    portable: bool,
    secrets: SecretStore,
    github_configured: AtomicBool,
    close_to_tray: AtomicBool,
    agent: Mutex<Option<RunningAgent>>,
    agent_error: Mutex<Option<String>>,
    settings_lock: Mutex<()>,
}

impl HostState {
    async fn snapshot(&self) -> Result<Value, String> {
        let mut snapshot = self.core.handle("snapshot", json!({})).await?;
        let agent = self.agent.lock().await;
        snapshot["runtime"] = json!({
            "appVersion": env!("CARGO_PKG_VERSION"),
            "dataDir": self.data_dir.to_string_lossy(),
            "portable": self.portable,
            "githubTokenConfigured": self.github_configured.load(Ordering::Relaxed),
            "agent": {
                "enabled": agent.is_some(),
                "address": agent.as_ref().map(|agent| &agent.address),
                "tokenConfigured": agent.is_some(),
                "error": self.agent_error.lock().await.clone()
            }
        });
        Ok(snapshot)
    }

    async fn configure_agent(&self, enabled: bool, port: u16, allow_new_secret: bool) -> Result<(), String> {
        let mut running = self.agent.lock().await;
        if !enabled {
            if let Some(previous) = running.take() { previous.stop().await; }
            *self.agent_error.lock().await = None;
            return Ok(());
        }
        if !(1024..=65535).contains(&port) { return Err("本机端口应在 1024 到 65535 之间。".into()); }
        if running.as_ref().is_some_and(|agent| agent.port == port) { return Ok(()); }
        let token = match self.secrets.get("agent")? {
            Some(token) => token,
            None if allow_new_secret => {
                let token = random_token();
                self.secrets.set("agent", Some(&token))?;
                token
            }
            None => return Err("这是另一台电脑的数据，或本机密钥已移除。请在设置中重新开启 Agent 接入。".into()),
        };
        // Bind the new port before stopping the old listener, preserving a working connection on failure.
        let next = agent::start(self.core.clone(), port, token).await?;
        if let Some(previous) = running.replace(next) { previous.stop().await; }
        *self.agent_error.lock().await = None;
        Ok(())
    }

    async fn save_settings(&self, changes: Value) -> Result<Value, String> {
        let _serial = self.settings_lock.lock().await;
        if !changes.is_object() { return Err("设置格式不正确。".into()); }
        let snapshot = self.core.handle("snapshot", json!({})).await?;
        let previous = snapshot["settings"].clone();
        let enabled = changes["agentEnabled"].as_bool().unwrap_or(previous["agentEnabled"].as_bool().unwrap_or(false));
        let port = changes.get("agentPort").unwrap_or(&previous["agentPort"]).as_u64().unwrap_or(47831);
        if !(1024..=65535).contains(&port) { return Err("本机端口应在 1024 到 65535 之间。".into()); }
        // The core validates/persists the complete settings; rollback if the listener cannot be configured.
        let result = self.core.handle("save_settings", json!({"settings": changes})).await?;
        if let Err(error) = self.configure_agent(enabled, port as u16, true).await {
            let _ = self.core.handle("save_settings", json!({"settings": previous})).await;
            *self.agent_error.lock().await = Some(error.clone());
            return Err(error);
        }
        self.close_to_tray.store(result["closeToTray"].as_bool().unwrap_or(false), Ordering::Relaxed);
        Ok(result)
    }

    async fn connection(&self) -> Result<Value, String> {
        let running = self.agent.lock().await;
        let agent = running.as_ref().ok_or("请先在设置中开启 Agent 接入。")?;
        let token = self.secrets.get("agent")?.ok_or("本机访问密钥不存在，请重新开启接入。")?;
        Ok(json!({"address": agent.address, "token": token}))
    }
}

fn object_string<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload[key].as_str().ok_or_else(|| format!("缺少必要信息：{key}"))
}

fn safe_browser_url(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "链接格式不正确。")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("只打开没有账户信息的安全 HTTPS 链接。".into());
    }
    let host = url.host_str().unwrap_or("");
    if !matches!(host, "github.com" | "raw.githubusercontent.com" | "docs.github.com" | "learn.microsoft.com" | "developer.microsoft.com" | "go.microsoft.com") {
        return Err("为避免误开未知网站，应用内只打开 GitHub 和微软官方链接。".into());
    }
    Ok(url.to_string())
}

fn managed_directory_from_versions(versions: &Value, download_id: &str, data_dir: &Path) -> Result<PathBuf, String> {
    let version = versions.as_array().and_then(|items| items.iter().find(|item| item["downloadId"].as_str() == Some(download_id)))
        .ok_or("这个版本没有受管安装记录，或目录已被移走。")?;
    let path = PathBuf::from(version["path"].as_str().ok_or("版本目录记录无效。")?)
        .canonicalize().map_err(|_| "版本目录已被移走或删除。")?;
    let root = data_dir.join("apps").canonicalize().map_err(|_| "受管目录不可用。")?;
    if !path.is_dir() || path == root || !path.starts_with(&root) {
        return Err("这个位置不属于 ReleaseHub 管理的程序目录。".into());
    }
    Ok(path)
}

#[tauri::command]
async fn dispatch(app: tauri::AppHandle, state: tauri::State<'_, Arc<HostState>>, action: String, payload: Option<Value>) -> Result<Value, String> {
    let host = state.inner().clone();
    let payload = payload.unwrap_or_else(|| json!({}));
    match action.as_str() {
        "snapshot" => host.snapshot().await,
        "save_settings" => {
            let value = host.save_settings(payload["settings"].clone()).await?;
            let _ = app.emit("releasehub:changed", json!({}));
            Ok(value)
        }
        "set_github_token" => {
            let token = object_string(&payload, "token")?.trim();
            if token.len() > 1024 || token.bytes().any(|byte| byte.is_ascii_control()) {
                return Err("授权内容格式不正确，请检查是否多复制了换行或其他内容。".into());
            }
            let value = if token.is_empty() { None } else { Some(token) };
            host.secrets.set("github", value)?;
            host.core.set_github_token(value.map(str::to_owned));
            host.github_configured.store(value.is_some(), Ordering::Relaxed);
            let _ = app.emit("releasehub:changed", json!({}));
            Ok(Value::Null)
        }
        "reveal_agent_connection" => host.connection().await,
        "rotate_agent_token" => {
            let _serial = host.settings_lock.lock().await;
            let running = host.agent.lock().await;
            let agent = running.as_ref().ok_or("请先开启 Agent 接入。")?;
            let token = random_token();
            host.secrets.set("agent", Some(&token))?;
            agent.rotate_token(token.clone())?;
            Ok(json!({"address": agent.address, "token": token}))
        }
        "pick_directory" => {
            let chosen = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
                .await.map_err(|_| "无法打开目录选择窗口。")?;
            let value = match chosen {
                Some(path) => Some(path.into_path().map_err(|_| "请选择本机目录。")?.to_string_lossy().to_string()),
                None => None,
            };
            Ok(json!(value))
        }
        "export_library_file" => {
            let contents = host.core.handle("export_library", json!({})).await?
                .as_str().ok_or("无法生成导出文件。")?.to_owned();
            let chosen = tauri::async_runtime::spawn_blocking(move || {
                app.dialog().file().add_filter("ReleaseHub 软件列表", &["json"]).set_file_name("ReleaseHub-subscriptions.json").blocking_save_file()
            }).await.map_err(|_| "无法打开保存窗口。")?;
            let Some(chosen) = chosen else { return Ok(Value::Null); };
            let path = chosen.into_path().map_err(|_| "请选择本机保存位置。")?;
            tokio::fs::write(&path, contents.as_bytes()).await.map_err(|_| "无法保存文件，请检查目标位置是否可写。")?;
            Ok(json!({"path": path.to_string_lossy()}))
        }
        "import_library_file" => {
            let chosen = tauri::async_runtime::spawn_blocking(move || app.dialog().file().add_filter("ReleaseHub 软件列表", &["json"]).blocking_pick_file())
                .await.map_err(|_| "无法打开文件选择窗口。")?;
            let Some(chosen) = chosen else { return Ok(Value::Null); };
            let path = chosen.into_path().map_err(|_| "请选择本机文件。")?;
            let metadata = tokio::fs::metadata(&path).await.map_err(|_| "无法读取所选文件。")?;
            if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 { return Err("请选择小于 2 MB 的软件列表 JSON 文件。".into()); }
            let contents = tokio::fs::read_to_string(path).await.map_err(|_| "文件不是有效的 UTF-8 软件列表。")?;
            host.core.handle("import_library", json!({"contents": contents})).await
        }
        "open_url" => {
            let url = safe_browser_url(object_string(&payload, "url")?)?;
            app.opener().open_url(url, None::<&str>).map_err(|_| "无法打开系统浏览器。")?;
            Ok(Value::Null)
        }
        "open_data_directory" => {
            app.opener().open_path(host.data_dir.to_string_lossy().to_string(), None::<&str>).map_err(|_| "无法打开数据文件夹。")?;
            Ok(Value::Null)
        }
        "open_managed_version" => {
            let project_id = object_string(&payload, "projectId")?;
            let download_id = object_string(&payload, "downloadId")?;
            let versions = host.core.handle("list_managed_versions", json!({"projectId": project_id})).await?;
            let path = managed_directory_from_versions(&versions, download_id, &host.data_dir)?;
            app.opener().open_path(path.to_string_lossy().to_string(), None::<&str>)
                .map_err(|_| "无法打开这个版本的程序目录。")?;
            Ok(Value::Null)
        }
        "open_download" => {
            let id = object_string(&payload, "id")?;
            let snapshot = host.core.handle("snapshot", json!({})).await?;
            let download = snapshot["downloads"].as_array().and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(id))).ok_or("下载记录不存在。")?;
            let stored = download["path"].as_str().ok_or("文件还没有下载完成。")?;
            let path = PathBuf::from(stored).canonicalize().map_err(|_| "下载文件已移动或删除。")?;
            let root = host.data_dir.canonicalize().map_err(|_| "数据目录不可用。")?;
            if !path.starts_with(&root) || !path.is_file() { return Err("下载记录的位置不在受管数据目录中。".into()); }
            app.opener().reveal_item_in_dir(path).map_err(|_| "无法在文件夹中显示下载文件。")?;
            Ok(Value::Null)
        }
        // The HTTP router never delegates arbitrary actions to this command.
        _ => host.core.handle(&action, payload).await,
    }
}

async fn scheduled_checks(app: tauri::AppHandle, host: Arc<HostState>) {
    let mut first = true;
    let mut previous_check = Instant::now();
    let mut notified = std::collections::HashMap::<String, String>::new();
    loop {
        tokio::time::sleep(if first { Duration::from_secs(8) } else { Duration::from_secs(30) }).await;
        // Idle scheduling reads a small settings row, not every release note and download.
        let Ok(settings) = host.core.handle("get_settings", json!({})).await else { continue };
        let minutes = settings["checkIntervalMinutes"].as_u64().unwrap_or(60);
        if minutes == 0 { first = false; continue; }
        if !first && previous_check.elapsed() < Duration::from_secs(minutes.saturating_mul(60)) { continue; }
        first = false;
        previous_check = Instant::now();
        let Ok(result) = host.core.handle("check_updates", json!({"allowAutoDownload": true})).await else { continue };
        if result["updates"].as_u64().unwrap_or(0) == 0 { continue; }
        let Ok(current_settings) = host.core.handle("get_settings", json!({})).await else { continue };
        if !current_settings["notifications"].as_bool().unwrap_or(true) { continue; }
        let Ok(after) = host.core.handle("snapshot", json!({})).await else { continue };
        let mut names = Vec::new();
        for project in after["projects"].as_array().into_iter().flatten() {
            let id = project["id"].as_str().unwrap_or_default();
            let tag = project["release"]["tag"].as_str().unwrap_or_default();
            if project["status"].as_str() == Some("updateAvailable") && notified.get(id).map(String::as_str) != Some(tag) {
                names.push(project["name"].as_str().unwrap_or("软件").to_owned());
                notified.insert(id.to_owned(), tag.to_owned());
            }
        }
        if !names.is_empty() {
            let title = format!("发现 {} 项软件更新", names.len());
            let body = names.into_iter().take(4).collect::<Vec<_>>().join("、");
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{menu::{Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}};
    let open = MenuItem::with_id(app, "open", "打开 ReleaseHub", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "检查软件更新", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &check, &quit])?;
    let mut tray = TrayIconBuilder::new().menu(&menu).tooltip("ReleaseHub · 追新").show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "check" => {
                if let Some(host) = app.try_state::<Arc<HostState>>() {
                    let core = host.core.clone();
                    tauri::async_runtime::spawn(async move { let _ = core.handle("check_updates", json!({})).await; });
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) { show_main(tray.app_handle()); }
        });
    if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
    tray.build(app)?;
    Ok(())
}

fn smoke_mode() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let network = args.get(1).map(String::as_str) == Some("--network-smoke-test");
    if !network && args.get(1).map(String::as_str) != Some("--smoke-test") { return false; }
    let Some(directory) = args.get(2) else { std::process::exit(2) };
    let result = (|| -> Result<(), String> {
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() { return Err("自测目录必须是完整路径。".into()); }
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let core = Core::new(directory.join("data"), true, Arc::new(|_, _| {}))?;
        let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
        let result = runtime.block_on(async {
            let checks = if network {
                for repository in ["Lake1059/FFmpegFreeUI", "RoutineMiku/apkstart", "Predidit/Kazumi"] {
                    core.handle("add_project", json!({"input":{"repository":repository,"channel":"stable","preference":"auto"}})).await?;
                }
                Some(core.handle("check_updates", json!({"allowAutoDownload": false})).await?)
            } else { None };
            let snapshot = core.handle("snapshot", json!({})).await?;
            let exported = core.handle("export_library", json!({})).await?;
            let ok = checks.as_ref().is_none_or(|check| check["errors"].as_u64() == Some(0));
            Ok::<Value, String>(json!({"ok":ok,"network":network,"checks":checks,"snapshot":snapshot,"export":exported}))
        })?;
        fs::write(directory.join("result.json"), serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        if result["ok"] != true { return Err("联网检查有失败项目，详情已保存在自测目录。".into()); }
        Ok(())
    })();
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            let _ = fs::write(Path::new(directory).join("error.txt"), error.as_bytes());
            std::process::exit(1)
        }
    }
}

#[cfg(windows)]
fn startup_message(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "ReleaseHub · 追新".encode_utf16().chain(Some(0)).collect();
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR); }
}
#[cfg(not(windows))]
fn startup_message(message: &str) { eprintln!("{message}"); }

pub fn run() {
    if smoke_mode() { return; }
    if tauri::webview_version().is_err() {
        startup_message("ReleaseHub 需要 Microsoft Edge WebView2 运行环境。\n请使用安装版自动补齐，或在微软官网安装 WebView2 后重试。\nhttps://developer.microsoft.com/microsoft-edge/webview2/");
        return;
    }
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![dispatch])
        .setup(|app| {
            let executable = std::env::current_exe()?;
            let parent = executable.parent().ok_or("无法定位应用目录。")?;
            let portable = parent.join("portable.flag").is_file();
            let data_dir = if portable { parent.join("data") } else { app.path().app_local_data_dir()? };
            fs::create_dir_all(&data_dir)?;
            let handle = app.handle().clone();
            let events = Arc::new(move |name: &str, value: Value| { let _ = handle.emit(name, value); });
            // Acquire the data lock before creating the instance identity or opening a WebView.
            let core = Core::new(data_dir.clone(), portable, events)?;
            let secrets = SecretStore::for_data_directory(&data_dir)?;
            let webview_dir = data_dir.join("webview");
            fs::create_dir_all(&webview_dir)?;
            let token = secrets.get("github").unwrap_or(None);
            let github_configured = token.is_some();
            core.set_github_token(token);
            let host = Arc::new(HostState {
                core, data_dir, portable, secrets,
                github_configured: AtomicBool::new(github_configured),
                close_to_tray: AtomicBool::new(false),
                agent: Mutex::new(None), agent_error: Mutex::new(None), settings_lock: Mutex::new(()),
            });
            app.manage(host.clone());
            let window_config = app.config().app.windows.iter()
                .find(|window| window.label == "main").ok_or("找不到主窗口设置。")?;
            tauri::WebviewWindowBuilder::from_config(app, window_config)?
                .data_directory(webview_dir)
                .build()?;
            setup_tray(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(snapshot) = host.core.handle("snapshot", json!({})).await {
                    host.close_to_tray.store(snapshot["settings"]["closeToTray"].as_bool().unwrap_or(false), Ordering::Relaxed);
                    if snapshot["settings"]["agentEnabled"].as_bool().unwrap_or(false) {
                        let port = snapshot["settings"]["agentPort"].as_u64().unwrap_or(47831) as u16;
                        if let Err(error) = host.configure_agent(true, port, false).await {
                            let _ = host.core.handle("save_settings", json!({"settings":{"agentEnabled":false}})).await;
                            *host.agent_error.lock().await = Some(error);
                        }
                    }
                }
                let _ = handle.emit("releasehub:changed", json!({}));
                scheduled_checks(handle, host).await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if let Some(host) = window.app_handle().try_state::<Arc<HostState>>() {
                    if host.close_to_tray.load(Ordering::Relaxed) { api.prevent_close(); let _ = window.hide(); }
                }
            }
        })
        .run(tauri::generate_context!());
    if let Err(error) = result { startup_message(&format!("ReleaseHub 无法启动：{error}\n请确认文件夹可写、文件已完整解压。")); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_directory_opener_accepts_only_recorded_local_folders() {
        let directory = tempfile::tempdir().unwrap();
        let managed = directory.path().join("apps").join("project").join("v1");
        fs::create_dir_all(&managed).unwrap();
        let versions = json!([{"downloadId":"one", "path":managed}]);
        assert_eq!(managed_directory_from_versions(&versions, "one", directory.path()).unwrap(), managed.canonicalize().unwrap());
        assert!(managed_directory_from_versions(&versions, "unknown", directory.path()).is_err());
        let outside = tempfile::tempdir().unwrap();
        assert!(managed_directory_from_versions(&json!([{"downloadId":"one","path":outside.path()}]), "one", directory.path()).is_err());
        let file = managed.join("app.exe");
        fs::write(&file, b"not executable").unwrap();
        assert!(managed_directory_from_versions(&json!([{"downloadId":"one","path":file}]), "one", directory.path()).is_err());
    }

    #[test]
    fn system_browser_never_opens_local_or_injected_urls() {
        assert!(safe_browser_url("https://github.com/huai-bot/releasehub/releases").is_ok());
        for url in ["file:///C:/Windows/system32/cmd.exe", "javascript:alert(1)", "https://github.com.evil.test/x", "https://token@github.com/a/b", "http://127.0.0.1:3000/private"] { assert!(safe_browser_url(url).is_err()); }
    }
}
