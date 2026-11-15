//! One in-memory proxy policy for HTTP requests and the bundled GitHub CLI.
//! Passwords are supplied by the host; this module never persists them.
use reqwest::{ClientBuilder, Proxy};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{net::IpAddr, sync::RwLock, time::{Duration, Instant}};
use tokio::process::Command;
use url::{Host, Url};

const LOOPBACK_BYPASS: &str = "localhost,127.0.0.1,127.0.0.0/8,::1";
const PROXY_ENV: [&str; 8] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "all_proxy", "no_proxy"];
const TEST_TARGET: &str = "https://api.github.com/";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProxySettings {
    pub mode: String,
    pub url: String,
    pub username: String,
}
impl Default for ProxySettings {
    fn default() -> Self { Self { mode: "system".into(), url: String::new(), username: String::new() } }
}
impl ProxySettings {
    pub fn validate(&self) -> Result<(), String> {
        if !["system", "manual", "direct"].contains(&self.mode.as_str()) { return Err("代理模式无效".into()); }
        if self.username.len() > 255 || self.username.chars().any(|c| c.is_control() || c == ':') {
            return Err("代理账号过长或含不支持的字符".into());
        }
        if !self.url.trim().is_empty() { endpoint(&self.url, None)?; }
        else if self.mode == "manual" { return Err("请填写手动代理地址，例如 http://127.0.0.1:7890".into()); }
        Ok(())
    }
}

struct Configuration { settings: ProxySettings, password: Option<String> }
/// Share this Arc with every network service. Build a fresh client for each new request.
/// Deliberately has no Debug/Serialize implementation: the lock contains a password.
pub struct ProxyState { current: RwLock<Configuration> }
impl ProxyState {
    pub fn new(settings: ProxySettings, password: Option<String>) -> Result<Self, String> {
        settings.validate()?;
        validate_password(&settings, password.as_deref())?;
        Ok(Self { current: RwLock::new(Configuration { settings, password }) })
    }
    pub fn configure(&self, settings: ProxySettings, password: Option<String>) -> Result<(), String> {
        settings.validate()?;
        validate_password(&settings, password.as_deref())?;
        *self.current.write().map_err(|_| "代理设置暂时不可用".to_string())? = Configuration { settings, password };
        Ok(())
    }
    pub fn settings(&self) -> Result<ProxySettings, String> {
        Ok(self.current.read().map_err(|_| "代理设置暂时不可用".to_string())?.settings.clone())
    }
    /// A settings-only save must not send the old password to a different proxy or account.
    pub(crate) fn configure_settings(&self, settings: ProxySettings) -> Result<(), String> {
        self.persist_settings(settings, || Ok(()))
    }
    pub(crate) fn persist_settings(&self, settings: ProxySettings, persist: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        settings.validate()?;
        let mut current = self.current.write().map_err(|_| "代理设置暂时不可用".to_string())?;
        let same_account = current.settings.url.trim() == settings.url.trim() && current.settings.username == settings.username;
        validate_password(&settings, if same_account { current.password.as_deref() } else { None })?;
        // On a database error the entire previous configuration, including its password, survives.
        persist()?;
        if !same_account { current.password = None; }
        current.settings = settings;
        Ok(())
    }
    pub(crate) fn set_password(&self, password: Option<String>) -> Result<(), String> {
        let mut current = self.current.write().map_err(|_| "代理设置暂时不可用".to_string())?;
        validate_password(&current.settings, password.as_deref())?;
        current.password = password;
        Ok(())
    }
    pub fn client_builder(&self) -> Result<ClientBuilder, String> { Ok(builder(self.resolve()?)) }
    pub fn apply_command(&self, command: &mut Command) -> Result<(), String> {
        let resolved = self.resolve()?;
        // These changes belong only to this child process, never the application's environment.
        for key in PROXY_ENV { command.env_remove(key); }
        let bypass = resolved.bypass.command_value();
        for key in ["NO_PROXY", "no_proxy"] { command.env(key, &bypass); }
        for (upper, lower, proxy) in [
            ("HTTP_PROXY", "http_proxy", resolved.http.as_ref().or(resolved.all.as_ref())),
            ("HTTPS_PROXY", "https_proxy", resolved.https.as_ref().or(resolved.all.as_ref())),
            ("ALL_PROXY", "all_proxy", resolved.all.as_ref()),
        ] {
            if let Some(proxy) = proxy { command.env(upper, proxy.as_str()).env(lower, proxy.as_str()); }
        }
        Ok(())
    }
    /// No user information is returned. Native WebViews should keep their own system/PAC mode.
    pub fn webview_proxy_url(&self) -> Result<Option<Url>, String> {
        let resolved = self.resolve()?;
        let mut proxy = resolved.https.or(resolved.all);
        if let Some(url) = &mut proxy {
            url.set_username("").map_err(|_| "代理地址无效".to_string())?;
            url.set_password(None).map_err(|_| "代理地址无效".to_string())?;
        }
        Ok(proxy)
    }
    pub async fn test(&self) -> Result<Value, String> { self.test_target(TEST_TARGET).await }
    async fn test_target(&self, target: &str) -> Result<Value, String> {
        let mode = self.settings()?.mode;
        let resolved = self.resolve()?;
        let source = resolved.source;
        // A proxy test never borrows a GitHub token, cookies, or any other account credential.
        let client = builder(resolved).user_agent(concat!("ReleaseHub/", env!("CARGO_PKG_VERSION"), " proxy-test"))
            .connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "无法创建代理测试连接".to_string())?;
        let started = Instant::now();
        let (ok, status, message) = match client.get(target).send().await {
            Ok(response) => {
                let status = response.status();
                let message = if status.is_success() { "连接成功".to_string() }
                    else if status.as_u16() == 407 { String::from("代理验证未通过，请检查账号和密码") }
                    else { format!("已连接到网站，但网站返回 HTTP {}", status.as_u16()) };
                (status.is_success(), Some(status.as_u16()), message)
            }
            Err(error) => (false, None, if error.is_timeout() { "连接超时，请检查代理地址和网络" }
                else { "连接失败，请检查代理地址、账号密码和网络" }.to_string()),
        };
        Ok(json!({"ok": ok, "mode": mode, "source": source, "target": target, "status": status,
            "latencyMs": started.elapsed().as_millis() as u64, "message": message}))
    }
    fn resolve(&self) -> Result<ResolvedProxy, String> {
        let current = self.current.read().map_err(|_| "代理设置暂时不可用".to_string())?;
        match current.settings.mode.as_str() {
            "direct" => Ok(ResolvedProxy::empty("direct")),
            "manual" => {
                let mut url = endpoint(&current.settings.url, None)?;
                if !current.settings.username.is_empty() {
                    url.set_username(&current.settings.username).map_err(|_| "代理账号格式无效".to_string())?;
                    url.set_password(Some(current.password.as_deref().unwrap_or(""))).map_err(|_| "代理密码格式无效".to_string())?;
                }
                Ok(ResolvedProxy { all: Some(url), ..ResolvedProxy::empty("manual") })
            }
            "system" => resolve_system(windows_proxy(), |key| std::env::var(key).ok()),
            _ => Err("代理模式无效".into()),
        }
    }
}
fn validate_password(settings: &ProxySettings, password: Option<&str>) -> Result<(), String> {
    if let Some(password) = password {
        if !password.is_empty() && settings.username.is_empty() { return Err("填写代理密码时，也请填写代理账号".into()); }
        if password.len() > 4096 || password.contains('\0') { return Err("代理密码过长或格式无效".into()); }
        if settings.url.trim().starts_with("socks5") && password.len() > 255 { return Err("SOCKS5 代理密码不能超过255字节".into()); }
    }
    Ok(())
}
fn endpoint(value: &str, default_scheme: Option<&str>) -> Result<Url, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) { return Err("代理地址为空、过长或含无效字符".into()); }
    let value = match default_scheme { Some(scheme) if !value.contains("://") => format!("{scheme}://{value}"), _ => value.into() };
    let url = Url::parse(&value).map_err(|_| "代理地址格式无效，请填写 http:// 或 socks5:// 开头的地址".to_string())?;
    if !["http", "https", "socks5", "socks5h"].contains(&url.scheme()) || url.host_str().is_none() || url.port() == Some(0)
        || !["", "/"].contains(&url.path()) || url.query().is_some() || url.fragment().is_some() {
        return Err("代理地址须为 HTTP 或 SOCKS5 地址，不能包含路径、查询或片段".into());
    }
    if !url.username().is_empty() || url.password().is_some() || value.split("://").nth(1).is_some_and(|s| s.split('/').next().is_some_and(|s| s.contains('@'))) {
        return Err("代理地址不能包含账号密码，请在单独的账号和密码框中填写".into());
    }
    Ok(url)
}
fn builder(resolved: ResolvedProxy) -> ClientBuilder {
    // no_proxy() prevents reqwest from silently re-reading process/system proxy settings.
    // The custom selector also bypasses loopback for authenticated SOCKS proxies.
    reqwest::Client::builder().no_proxy().proxy(Proxy::custom(move |url| resolved.for_url(url).cloned()))
}

struct ResolvedProxy { http: Option<Url>, https: Option<Url>, all: Option<Url>, bypass: Bypass, source: &'static str }
impl ResolvedProxy {
    fn empty(source: &'static str) -> Self { Self { http: None, https: None, all: None, bypass: Bypass::new(""), source } }
    fn for_url(&self, url: &Url) -> Option<&Url> {
        if self.bypass.matches(url) { return None; }
        match url.scheme() { "http" => self.http.as_ref().or(self.all.as_ref()), "https" => self.https.as_ref().or(self.all.as_ref()), _ => None }
    }
}
struct WindowsProxy { server: String, bypass: String }
#[cfg(windows)]
fn windows_proxy() -> Option<WindowsProxy> {
    use winreg::{enums::{HKEY_CURRENT_USER, KEY_READ}, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", KEY_READ).ok()?;
    if key.get_value::<u32, _>("ProxyEnable").ok()? == 0 { return None; }
    let server: String = key.get_value("ProxyServer").ok()?;
    if server.trim().is_empty() { return None; }
    Some(WindowsProxy { server, bypass: key.get_value("ProxyOverride").unwrap_or_default() })
}
#[cfg(not(windows))]
fn windows_proxy() -> Option<WindowsProxy> { None }
fn resolve_system(windows: Option<WindowsProxy>, lookup: impl Fn(&str) -> Option<String>) -> Result<ResolvedProxy, String> {
    let env = |upper: &str, lower: &str| lookup(upper).filter(|s| !s.trim().is_empty()).or_else(|| lookup(lower).filter(|s| !s.trim().is_empty()));
    let bypass = env("NO_PROXY", "no_proxy").unwrap_or_default();
    if let Some(windows) = windows {
        let mut resolved = ResolvedProxy::empty("windows");
        resolved.bypass = Bypass::new(&format!("{};{}", windows.bypass, bypass));
        if !windows.server.contains('=') { resolved.all = Some(endpoint(&windows.server, Some("http"))?); }
        else {
            for entry in windows.server.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                let (scheme, address) = entry.split_once('=').ok_or("Windows 系统代理格式无效，请在设置中改用手动代理")?;
                match scheme.trim().to_ascii_lowercase().as_str() {
                    "http" => resolved.http = Some(endpoint(address, Some("http"))?),
                    "https" => resolved.https = Some(endpoint(address, Some("http"))?),
                    "socks" | "socks5" => resolved.all = Some(endpoint(address, Some("socks5"))?),
                    _ => {}, // FTP and other Windows-only protocols are not HTTP routes.
                }
            }
        }
        return Ok(resolved);
    }
    let mut resolved = ResolvedProxy::empty("environment");
    resolved.http = env("HTTP_PROXY", "http_proxy").map(|s| endpoint(&s, Some("http"))).transpose()?;
    resolved.https = env("HTTPS_PROXY", "https_proxy").map(|s| endpoint(&s, Some("http"))).transpose()?;
    resolved.all = env("ALL_PROXY", "all_proxy").map(|s| endpoint(&s, Some("http"))).transpose()?;
    resolved.bypass = Bypass::new(&bypass);
    if resolved.http.is_none() && resolved.https.is_none() && resolved.all.is_none() { resolved.source = "direct"; }
    Ok(resolved)
}
struct Bypass { entries: Vec<String> }
impl Bypass {
    fn new(value: &str) -> Self {
        Self { entries: value.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_ascii_lowercase).collect() }
    }
    fn command_value(&self) -> String {
        if self.entries.iter().any(|s| s == "*") { return "*".into(); }
        let mut entries = vec![LOOPBACK_BYPASS.to_string()];
        entries.extend(self.entries.iter().filter(|s| s.as_str() != "<local>")
            .map(|s| if s.starts_with("*.") { s[1..].to_string() } else { s.to_string() }));
        entries.join(",")
    }
    fn matches(&self, url: &Url) -> bool {
        let Some(host) = url.host() else { return false; };
        match host {
            Host::Ipv4(ip) if ip.is_loopback() => return true,
            Host::Ipv6(ip) if ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback()) => return true,
            Host::Domain(name) if name.eq_ignore_ascii_case("localhost") || name.ends_with(".localhost") => return true,
            _ => {},
        }
        let hostname = url.host_str().unwrap_or("").trim_matches(['[', ']']).to_ascii_lowercase();
        self.entries.iter().any(|entry| {
            if entry == "*" { return true; }
            if entry == "<local>" { return matches!(host, Host::Domain(name) if !name.contains('.')); }
            let mut pattern = entry.as_str();
            if let Some((network, prefix)) = pattern.split_once('/') {
                return cidr_match(&hostname, network, prefix);
            }
            if (pattern.starts_with('[') && pattern.contains("]:")) || pattern.matches(':').count() == 1 {
                if let Some((name, port)) = pattern.rsplit_once(':') {
                    if let Ok(port) = port.parse::<u16>() {
                        if url.port_or_known_default() != Some(port) { return false; }
                        pattern = name;
                    }
                }
            }
            let pattern = pattern.trim_matches(['[', ']']).trim_start_matches("*.").trim_start_matches('.');
            hostname == pattern || hostname.ends_with(&format!(".{pattern}"))
        })
    }
}
fn cidr_match(host: &str, network: &str, prefix: &str) -> bool {
    match (host.parse::<IpAddr>(), network.parse::<IpAddr>(), prefix.parse::<u32>()) {
        (Ok(IpAddr::V4(host)), Ok(IpAddr::V4(network)), Ok(prefix)) if prefix <= 32 => prefix == 0 || (u32::from(host) >> (32 - prefix)) == (u32::from(network) >> (32 - prefix)),
        (Ok(IpAddr::V6(host)), Ok(IpAddr::V6(network)), Ok(prefix)) if prefix <= 128 => prefix == 0 || (u128::from(host) >> (128 - prefix)) == (u128::from(network) >> (128 - prefix)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};
    fn manual(url: String, username: &str) -> ProxySettings { ProxySettings { mode: "manual".into(), url, username: username.into() } }
    fn direct() -> ProxySettings { ProxySettings { mode: "direct".into(), ..ProxySettings::default() } }
    async fn read_headers(stream: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        loop {
            bytes.push(stream.read_u8().await.unwrap());
            if bytes.ends_with(b"\r\n\r\n") { break; }
            assert!(bytes.len() < 16384);
        }
        String::from_utf8(bytes).unwrap()
    }
    async fn http_fixture(body: &'static str, status: u16) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let work = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let headers = read_headers(&mut stream).await;
            stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            headers
        });
        (address, work)
    }
    #[test]
    fn validates_settings_without_accepting_embedded_secrets() {
        for address in ["http://fixture:secret@127.0.0.1:9", "socks5://fixture@127.0.0.1:9", "http://@127.0.0.1:9", "ftp://127.0.0.1:9", "http://127.0.0.1/path", "http://127.0.0.1:0", "http://127.0.0.1?password=x"] {
            assert!(manual(address.into(), "").validate().is_err());
        }
        for address in ["http://127.0.0.1:9", "https://proxy.example:443", "socks5://[::1]:1080", "socks5h://127.0.0.1:1080"] {
            assert!(manual(address.into(), "fixture-user").validate().is_ok());
        }
        assert!(serde_json::from_value::<ProxySettings>(json!({"password":"synthetic-only"})).is_err());
        assert!(ProxyState::new(manual("http://127.0.0.1:9".into(), ""), Some("synthetic-only".into())).is_err());
        assert!(ProxyState::new(manual("http://127.0.0.1:9".into(), ""), None).is_ok());
    }
    #[test]
    fn windows_proxy_wins_and_environment_is_the_fallback() {
        let lookup = |key: &str| match key { "HTTPS_PROXY" => Some("http://127.0.0.1:8200".into()), "NO_PROXY" => Some("env.example".into()), _ => None };
        let resolved = resolve_system(Some(WindowsProxy { server: "http=127.0.0.1:8100;https=127.0.0.1:8101;socks=127.0.0.1:8102".into(), bypass: "*.internal.example;<local>".into() }), lookup).unwrap();
        assert_eq!(resolved.source, "windows");
        assert_eq!(resolved.for_url(&Url::parse("https://public.example").unwrap()).unwrap().port(), Some(8101));
        assert_eq!(resolved.for_url(&Url::parse("http://public.example").unwrap()).unwrap().port(), Some(8100));
        for target in ["http://127.0.0.1", "http://127.5.6.7", "http://[::1]", "http://localhost", "http://printer", "https://app.internal.example", "https://env.example"] {
            assert!(resolved.for_url(&Url::parse(target).unwrap()).is_none());
        }
        let fallback = resolve_system(None, lookup).unwrap();
        assert_eq!(fallback.source, "environment");
        assert_eq!(fallback.https.unwrap().port(), Some(8200));
        assert!(Bypass::new("10.0.0.0/8").matches(&Url::parse("http://10.20.30.40").unwrap()));
    }
    #[test]
    fn failed_settings_save_preserves_the_previous_proxy_and_password() {
        let settings = manual("http://127.0.0.1:9".into(), "fixture-user");
        let state = ProxyState::new(settings.clone(), Some("fixture-only-password".into())).unwrap();
        let next = manual("http://127.0.0.1:10".into(), "another-fixture-user");
        assert!(state.persist_settings(next, || Err("fixture storage error".into())).is_err());
        assert_eq!(state.settings().unwrap(), settings);
        assert_eq!(state.current.read().unwrap().password.as_deref(), Some("fixture-only-password"));
        state.configure_settings(direct()).unwrap();
        assert!(state.current.read().unwrap().password.is_none());
    }
    #[tokio::test]
    async fn manual_http_proxy_auth_and_changes_apply_to_new_clients() {
        let (first, first_work) = http_fixture("first", 200).await;
        let state = ProxyState::new(manual(first, "fixture-user"), Some("fixture-only-password".into())).unwrap();
        let response = state.client_builder().unwrap().timeout(Duration::from_secs(3)).build().unwrap().get("http://proxy-target.invalid/check").send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "first");
        let headers = first_work.await.unwrap().to_ascii_lowercase();
        assert!(headers.starts_with("get http://proxy-target.invalid/check http/1.1"));
        assert!(headers.contains("\r\nproxy-authorization: basic "));
        assert!(!headers.contains("\r\nauthorization:"));
        let (second, second_work) = http_fixture("second", 200).await;
        state.configure_settings(manual(second, "fixture-user")).unwrap();
        assert!(state.current.read().unwrap().password.is_none());
        let response = state.client_builder().unwrap().timeout(Duration::from_secs(3)).build().unwrap().get("http://proxy-target.invalid/check").send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "second");
        second_work.await.unwrap();
    }
    #[tokio::test]
    async fn loopback_bypasses_an_authenticated_manual_proxy() {
        let (origin, work) = http_fixture("local", 200).await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let state = ProxyState::new(manual(format!("http://{}", proxy_listener.local_addr().unwrap()), "fixture-user"), Some("fixture-only-password".into())).unwrap();
        let response = state.client_builder().unwrap().timeout(Duration::from_secs(3)).build().unwrap().get(origin).send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "local");
        assert!(!work.await.unwrap().to_ascii_lowercase().contains("authorization:"));
        assert!(tokio::time::timeout(Duration::from_millis(30), proxy_listener.accept()).await.is_err());
    }
    #[tokio::test]
    async fn socks5_auth_stays_at_the_local_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let state = ProxyState::new(manual(format!("socks5h://{}", listener.local_addr().unwrap()), "fixture-user"), Some("fixture-password".into())).unwrap();
        let work = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(stream.read_u8().await.unwrap(), 5);
            let count = stream.read_u8().await.unwrap(); let mut methods = vec![0; count as usize]; stream.read_exact(&mut methods).await.unwrap();
            assert!(methods.contains(&2)); stream.write_all(&[5, 2]).await.unwrap();
            assert_eq!(stream.read_u8().await.unwrap(), 1);
            let count = stream.read_u8().await.unwrap(); let mut user = vec![0; count as usize]; stream.read_exact(&mut user).await.unwrap();
            let count = stream.read_u8().await.unwrap(); let mut password = vec![0; count as usize]; stream.read_exact(&mut password).await.unwrap();
            assert_eq!(user, b"fixture-user"); assert_eq!(password, b"fixture-password"); stream.write_all(&[1, 0]).await.unwrap();
            let mut request = [0; 4]; stream.read_exact(&mut request).await.unwrap(); assert_eq!(request, [5, 1, 0, 3]);
            let count = stream.read_u8().await.unwrap(); let mut domain = vec![0; count as usize]; stream.read_exact(&mut domain).await.unwrap();
            assert_eq!(domain, b"socks-target.invalid"); assert_eq!(stream.read_u16().await.unwrap(), 80);
            stream.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await.unwrap();
            let headers = read_headers(&mut stream).await; assert!(!headers.to_ascii_lowercase().contains("authorization:"));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nsocks").await.unwrap();
        });
        let response = state.client_builder().unwrap().timeout(Duration::from_secs(3)).build().unwrap().get("http://socks-target.invalid/").send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "socks"); work.await.unwrap();
    }
    #[test]
    fn command_policy_is_local_and_direct_clears_inherited_overrides() {
        let state = ProxyState::new(manual("http://127.0.0.1:9".into(), "fixture-user"), Some("fixture-only-password".into())).unwrap();
        let mut command = Command::new("not-executed-fixture");
        command.env("HTTP_PROXY", "http://stale.invalid:9").env("NO_PROXY", "*"); state.apply_command(&mut command).unwrap();
        let variables: std::collections::HashMap<_, _> = command.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().to_ascii_uppercase(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert!(variables["HTTP_PROXY"].as_ref().unwrap().contains("fixture-user:"));
        assert!(variables["NO_PROXY"].as_ref().unwrap().contains("127.0.0.0/8"));
        assert!(command.as_std().get_args().next().is_none());
        state.configure(direct(), None).unwrap(); state.apply_command(&mut command).unwrap();
        for (key, value) in command.as_std().get_envs() {
            if ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"].contains(&key.to_string_lossy().to_ascii_uppercase().as_str()) { assert!(value.is_none()); }
        }
    }
    #[tokio::test]
    async fn proxy_test_returns_status_without_any_credentials() {
        let (proxy, work) = http_fixture("", 407).await;
        let state = ProxyState::new(manual(proxy, "fixture-user"), Some("fixture-only-password".into())).unwrap();
        let result = state.test_target("http://test-target.invalid/").await.unwrap();
        work.await.unwrap(); assert_eq!(result["ok"], false); assert_eq!(result["status"], 407);
        assert!(!result.to_string().contains("fixture-user")); assert!(!result.to_string().contains("fixture-only-password"));
        assert!(state.webview_proxy_url().unwrap().unwrap().username().is_empty());
    }
}
