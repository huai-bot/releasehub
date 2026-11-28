use serde::{Deserialize, Serialize};

pub fn now() -> String { chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true) }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAsset {
    pub id: u64, pub name: String, pub size: u64, pub url: String,
    pub content_type: String, pub digest: Option<String>, pub kind: String,
    pub compatible: bool, pub score: i32, pub reason: String,
    #[serde(default)]
    pub downloadable: bool,
    #[serde(default)]
    pub installable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub id: u64, pub tag: String, pub name: String, pub body: String,
    pub published_at: String, pub url: String, pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String, pub owner: String, pub repo: String, pub name: String,
    pub description: String, pub url: String, pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_match: Option<String>,
    pub channel: String, pub preference: String, pub asset_pattern: Option<String>,
    #[serde(default = "default_management_mode")]
    pub management_mode: String,
    #[serde(default)]
    pub file_status: Option<String>,
    pub installed_version: Option<String>, pub installed_path: Option<String>,
    #[serde(default)]
    pub executable_path: Option<String>,
    #[serde(default)]
    pub backup_enabled: bool,
    #[serde(default)]
    pub updates_muted: bool,
    pub installed_source: Option<String>, pub release: Option<Release>,
    pub selected_asset_id: Option<u64>, pub status: String,
    pub checked_at: Option<String>, pub error: Option<String>, pub default_branch: String,
    pub source_head_sha: Option<String>, pub source_ahead: Option<bool>, pub created_at: String,
}
impl Project {
    pub fn available_update_tag(&self) -> Option<&str> {
        let available = if self.management_mode == "files" {
            self.file_status.as_deref() == Some("updateAvailable")
        } else { self.status == "updateAvailable" };
        self.release.as_ref().filter(|_| available && !self.updates_muted).map(|release| release.tag.as_str())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateVersion { pub project_id: String, pub tag: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "DownloadRecordWire")]
pub struct DownloadRecord {
    pub id: String, pub project_id: String, pub project_name: String, pub asset_id: u64,
    pub file_name: String, pub tag: String, pub url: String, pub kind: String,
    pub installable: bool,
    pub total_bytes: u64, pub received_bytes: u64, pub status: String,
    pub path: Option<String>, pub sha256: Option<String>, pub verified: bool,
    pub error: Option<String>, pub created_at: String, pub updated_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadRecordWire {
    id: String, project_id: String, project_name: String, asset_id: u64,
    file_name: String, tag: String, url: String, kind: String,
    #[serde(default)]
    installable: Option<bool>,
    total_bytes: u64, received_bytes: u64, status: String,
    path: Option<String>, sha256: Option<String>, verified: bool,
    error: Option<String>, created_at: String, updated_at: String,
}
impl From<DownloadRecordWire> for DownloadRecord {
    fn from(d: DownloadRecordWire) -> Self {
        let installable = d.installable.unwrap_or_else(|| ["installer", "portable"].contains(&d.kind.as_str()));
        Self { id:d.id, project_id:d.project_id, project_name:d.project_name, asset_id:d.asset_id,
            file_name:d.file_name, tag:d.tag, url:d.url, kind:d.kind, installable,
            total_bytes:d.total_bytes, received_bytes:d.received_bytes, status:d.status, path:d.path,
            sha256:d.sha256, verified:d.verified, error:d.error, created_at:d.created_at, updated_at:d.updated_at }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallationStatus {
    Waiting,
    Detecting,
    Verified,
    Manual,
    RebootRequired,
    Unconfirmed,
    Cancelled,
    Failed,
    Superseded,
}
impl InstallationStatus {
    pub(super) fn can_check(self) -> bool {
        matches!(self, Self::Waiting | Self::Detecting | Self::RebootRequired | Self::Unconfirmed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationCheck {
    pub id: String,
    pub project_id: String,
    pub download_id: String,
    pub expected_version: String,
    pub previous_version: Option<String>,
    pub status: InstallationStatus,
    pub started_at: String,
    pub checked_at: Option<String>,
    pub detected_version: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedVersion {
    pub download_id: String, pub version: String, pub path: String, pub active: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTask {
    pub id: String, pub project_id: String, pub project_name: String, pub repository: String,
    pub source_ref: String, pub source_commit: Option<String>, pub target: String,
    pub instructions: String, pub status: String, pub agent_name: Option<String>,
    pub progress: Option<String>, pub artifact_url: Option<String>,
    pub artifact_sha256: Option<String>, pub error: Option<String>,
    pub created_at: String, pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryNote {
    pub repository: String, pub name: String, pub description: String, pub updated_at: String,
}
impl RepositoryNote {
    pub fn normalized(mut self) -> Result<Self, String> {
        let (owner, repo) = super::network::repository(&self.repository)?;
        self.repository = format!("{owner}/{repo}").to_ascii_lowercase();
        self.name = note_text(&self.name, 80, "本地名称")?;
        self.description = note_text(&self.description, 240, "本地描述")?;
        if self.updated_at.len() > 64 || chrono::DateTime::parse_from_rfc3339(&self.updated_at).is_err() {
            return Err("备注更新时间无效".into());
        }
        Ok(self)
    }
}
fn note_text(value: &str, max: usize, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > max || value.chars().any(|c| c.is_control()) {
        return Err(format!("{label}最多{max}个字符，不能含换行或控制字符"));
    }
    Ok(value.to_owned())
}
pub const NAVIGATION_PAGES: [&str; 7] = ["home", "library", "updates", "downloads", "github", "trainers", "agent"];
fn default_library_sort() -> String { "added".into() }
fn read_navigation_order<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Vec<String>, D::Error> {
    let mut order = Vec::<String>::deserialize(de)?;
    upgrade_navigation_order(&mut order);
    Ok(order)
}
pub(super) fn upgrade_navigation_order(order: &mut Vec<String>) {
    // Upgrade only a valid v0.6 list. Invalid/duplicate page IDs still fail validation.
    if order.len() == 6 && NAVIGATION_PAGES.iter().filter(|&&p| p != "trainers").all(|p| order.iter().any(|id| id == p)) {
        let pos = order.iter().position(|id| id == "github").unwrap() + 1;
        order.insert(pos, "trainers".into());
    }
}
pub fn default_management_mode() -> String { "software".into() }
pub fn default_true() -> bool { true }
pub fn default_download_connections() -> usize { 4 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalDownloader {
    pub name: String, pub endpoint: String, pub request_template: String,
}
impl Default for ExternalDownloader {
    fn default() -> Self { Self { name: "AB Download Manager".into(), endpoint: "http://127.0.0.1:15151/add".into(),
        request_template: r#"{"items":[{"type":"http","link":"{url}","downloadPage":"{pageUrl}","suggestedName":"{fileName}"}],"options":{"silentAdd":true,"silentStart":true}}"#.into() } }
}
impl ExternalDownloader {
    pub fn migrate_legacy_ab(&mut self) {
        let legacy=serde_json::json!({"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":true});
        if self.name=="AB Download Manager" && self.endpoint=="http://127.0.0.1:15151/start-headless-download"
            && serde_json::from_str::<serde_json::Value>(&self.request_template).ok().as_ref()==Some(&legacy) {
            *self=Self::default();
        }
    }
}
pub fn default_color_scheme() -> String { "blue".into() }
pub fn default_material() -> String { "acrylic".into() }
pub fn default_blur() -> bool { true }
pub fn default_library_layout() -> String { "three".into() }
pub fn validate_management_mode(mode: &str) -> Result<(), String> {
    if !["software", "files"].contains(&mode) { return Err("管理模式无效".into()); }
    Ok(())
}
pub fn require_software(p: &Project) -> Result<(), String> {
    if p.management_mode != "software" { return Err("文件项目只管理下载，不能安装或记录已安装版本".into()); }
    Ok(())
}
pub fn default_startup_page() -> String { "home".into() }
pub fn default_startup_github_tab() -> String { "explore".into() }
pub fn default_navigation_order() -> Vec<String> { NAVIGATION_PAGES.iter().map(|s| (*s).into()).collect() }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub theme: String, pub check_interval_minutes: u32, pub notifications: bool,
    #[serde(default = "default_color_scheme")]
    pub color_scheme: String,
    #[serde(default = "default_material")]
    pub material: String,
    #[serde(default = "default_blur")]
    pub blur: bool,
    #[serde(default = "default_library_layout")]
    pub library_layout: String,
    #[serde(default = "default_library_sort")]
    pub library_sort: String,
    pub auto_download: bool, pub max_concurrent_downloads: usize, pub close_to_tray: bool,
    #[serde(default = "default_download_connections")]
    pub download_connections: usize,
    #[serde(default)]
    pub use_external_downloader: bool,
    #[serde(default)]
    pub external_downloader: ExternalDownloader,
    pub agent_enabled: bool, pub agent_port: u16,
    #[serde(default = "default_startup_page")]
    pub startup_page: String,
    #[serde(default = "default_startup_github_tab")]
    pub startup_github_tab: String,
    #[serde(default = "default_navigation_order", deserialize_with = "read_navigation_order")]
    pub navigation_order: Vec<String>,
    #[serde(default)]
    pub proxy: crate::proxy::ProxySettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self { theme: "system".into(), color_scheme: default_color_scheme(), material: default_material(), blur: default_blur(), library_layout: default_library_layout(), library_sort: default_library_sort(), check_interval_minutes: 60, notifications: true,
            auto_download: false, max_concurrent_downloads: 2, close_to_tray: true,
            download_connections: default_download_connections(), use_external_downloader: false, external_downloader: ExternalDownloader::default(),
            agent_enabled: false, agent_port: 47831, startup_page: default_startup_page(),
            startup_github_tab: default_startup_github_tab(), navigation_order: default_navigation_order(),
            proxy: crate::proxy::ProxySettings::default() }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !["system", "light", "dark"].contains(&self.theme.as_str()) { return Err("主题设置无效".into()); }
        if !["blue", "teal", "violet", "amber"].contains(&self.color_scheme.as_str()) { return Err("配色设置无效".into()); }
        if !["acrylic", "mica"].contains(&self.material.as_str()) { return Err("材质设置无效".into()); }
        if !["two", "three", "list"].contains(&self.library_layout.as_str()) { return Err("资源库布局无效".into()); }
        if !["added", "name", "nameDesc", "released", "updated"].contains(&self.library_sort.as_str()) { return Err("资源排序方式无效".into()); }
        if self.check_interval_minutes != 0 && !(5..=10080).contains(&self.check_interval_minutes) { return Err("检查间隔须为0（仅手动检查）或5至10080分钟".into()); }
        if !(1..=4).contains(&self.max_concurrent_downloads) { return Err("同时下载数量须为1至4".into()); }
        if !(1..=8).contains(&self.download_connections) { return Err("单文件连接数须为1至8".into()); }
        super::external_downloads::validate_config(&self.external_downloader)?;
        if self.agent_port < 1024 { return Err("Agent端口须为1024至65535".into()); }
        if !NAVIGATION_PAGES.contains(&self.startup_page.as_str()) { return Err("启动页面无效".into()); }
        if !["explore", "owned", "starred", "activity", "feed"].contains(&self.startup_github_tab.as_str()) { return Err("GitHub 启动页面无效".into()); }
        if self.navigation_order.len() != NAVIGATION_PAGES.len()
            || NAVIGATION_PAGES.iter().any(|page| self.navigation_order.iter().filter(|item| item.as_str() == *page).count() != 1) {
            return Err("导航顺序须包含每个工作区页面，不能重复".into());
        }
        self.proxy.validate()?;
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddProjectInput {
    pub repository: String, pub channel: String, pub preference: String,
    #[serde(default = "default_management_mode")]
    pub management_mode: String,
    pub installed_version: Option<String>, pub installed_path: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChanges {
    pub channel: Option<String>, pub preference: Option<String>,
    pub management_mode: Option<String>,
    pub backup_enabled: Option<bool>,
    pub updates_muted: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub asset_pattern: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub installed_version: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub installed_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub selected_asset_id: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub executable_path: Option<Option<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub download_id: String, pub project_id: String, pub target_path: String, pub backup_path: Option<String>,
    pub previous_version: Option<String>, pub next_version: String, pub backup_bytes: u64,
    pub package_bytes: u64, pub available_bytes: u64, pub kind: String, pub archive_roots: Vec<String>,
    pub suggested_root: Option<String>, pub requires_root_selection: bool, pub legacy_managed: bool,
    pub backup_enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub project_id: String, pub download_id: String, pub version: Option<String>,
    pub installed_source: Option<String>, pub target_path: String, pub path: String,
    pub created_at: String, pub bytes: u64, pub kind: String, pub phase: String, pub error: Option<String>,
}
/// Durable recovery state; private fields never form part of the shared bridge contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallJournal {
    pub backup: BackupRecord, pub original_project: Project, pub previous_download: DownloadRecord,
    pub target_empty: bool, pub stage_path: String,
    #[serde(default = "default_true")]
    pub snapshot_enabled: bool,
    #[serde(default)]
    pub snapshot_files: Vec<SnapshotEntry>,
    #[serde(default)]
    pub original_files: Vec<SnapshotEntry>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub path: String, pub directory: bool, pub bytes: u64, pub sha256: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallInput {
    pub id: String, pub confirmed: bool, pub target_path: String, pub archive_root: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub previous_version: Option<Option<String>>,
}
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> { Option::<T>::deserialize(d).map(Some) }

pub fn validate_choices(channel: &str, preference: &str) -> Result<(), String> {
    if !["stable", "preview"].contains(&channel) { return Err("版本通道无效".into()); }
    if !["auto", "portable", "installer"].contains(&preference) { return Err("安装包偏好无效".into()); }
    Ok(())
}

#[cfg(test)]
pub fn test_project(owner:&str,repo:&str)->Project {
    Project{id:uuid::Uuid::new_v4().to_string(),owner:owner.into(),repo:repo.into(),name:repo.into(),description:String::new(),
        url:format!("https://github.com/{owner}/{repo}"),avatar_url:None,repository_match:None,channel:"stable".into(),preference:"auto".into(),
        management_mode:default_management_mode(),file_status:None,asset_pattern:None,installed_version:None,installed_path:None,executable_path:None,
        backup_enabled:false,updates_muted:false,
        installed_source:None,release:None,
        selected_asset_id:None,status:"notChecked".into(),checked_at:None,error:None,default_branch:"main".into(),
        source_head_sha:None,source_ahead:None,created_at:now()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn colors_and_legacy_defaults_are_validated() {
        let mut settings=Settings::default();
        for color in ["blue","teal","violet","amber"] {settings.color_scheme=color.into();settings.validate().unwrap();}
        for color in ["", "red", "BLUE"] {settings.color_scheme=color.into();assert!(settings.validate().is_err());}
        let mut old=serde_json::to_value(Settings::default()).unwrap();old.as_object_mut().unwrap().remove("colorScheme");
        assert_eq!(serde_json::from_value::<Settings>(old).unwrap().color_scheme,"blue");
        let mut old=serde_json::to_value(test_project("owner","repo")).unwrap();
        for field in ["managementMode","fileStatus"] {old.as_object_mut().unwrap().remove(field);}
        let project:Project=serde_json::from_value(old).unwrap();assert_eq!(project.management_mode,"software");assert!(project.file_status.is_none());
        for kind in ["portable","installer","other"] {
            let mut old=serde_json::json!({"id":"test","projectId":"p","projectName":"p","assetId":1,"fileName":"file",
                "tag":"v1","url":"https://github.com/owner/repo/releases/download/v1/file","kind":kind,
                "totalBytes":1,"receivedBytes":1,"status":"completed","verified":false,"createdAt":now(),"updatedAt":now()});
            assert_eq!(serde_json::from_value::<DownloadRecord>(old.clone()).unwrap().installable,kind!="other");
            old["installable"]=serde_json::json!(false);
            assert!(!serde_json::from_value::<DownloadRecord>(old).unwrap().installable);
        }
    }
    #[test] fn glass_preferences_default_validate_and_roundtrip() {
        let defaults=Settings::default();
        assert_eq!(defaults.material,"acrylic"); assert!(defaults.blur);
        let mut old=serde_json::to_value(&defaults).unwrap();
        for field in ["material","blur"] {old.as_object_mut().unwrap().remove(field);}
        let restored:Settings=serde_json::from_value(old).unwrap();
        assert_eq!(restored.material,"acrylic"); assert!(restored.blur);
        for material in ["acrylic","mica"] {
            for blur in [true,false] {
                let mut settings=defaults.clone(); settings.material=material.into(); settings.blur=blur;
                settings.validate().unwrap();
                let restored:Settings=serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
                assert_eq!(restored.material,material); assert_eq!(restored.blur,blur);
            }
        }
        for material in ["","glass","ACRYLIC"] {
            let mut settings=defaults.clone(); settings.material=material.into();
            assert_eq!(settings.validate().unwrap_err(),"材质设置无效");
        }
    }
    #[test] fn library_layout_choices_are_validated_and_roundtrip() {
        let defaults=Settings::default(); assert_eq!(defaults.library_layout,"three");
        for layout in ["two","three","list"] {
            let mut settings=defaults.clone(); settings.library_layout=layout.into(); settings.validate().unwrap();
            let value=serde_json::to_value(&settings).unwrap(); assert_eq!(value["libraryLayout"],layout);
            let restored:Settings=serde_json::from_value(value).unwrap(); assert_eq!(restored.library_layout,layout);
        }
        for layout in ["","grid","TWO"] {
            let mut settings=defaults.clone(); settings.library_layout=layout.into();
            assert_eq!(settings.validate().unwrap_err(),"资源库布局无效");
        }
    }
    #[test] fn legacy_ab_defaults_migrate_despite_json_format_and_key_order() {
        for template in [
            r#"{"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":true}"#,
            r#"{
                "startDownload": true,
                "name": "{fileName}",
                "downloadSource": { "downloadPage": "{pageUrl}", "link": "{url}" }
            }"#,
        ] {
            let mut config=ExternalDownloader { name:"AB Download Manager".into(),
                endpoint:"http://127.0.0.1:15151/start-headless-download".into(),request_template:template.into() };
            config.migrate_legacy_ab();
            assert_eq!(config.name,"AB Download Manager"); assert_eq!(config.endpoint,"http://127.0.0.1:15151/add");
            assert_eq!(serde_json::from_str::<serde_json::Value>(&config.request_template).unwrap(),serde_json::json!({
                "items":[{"type":"http","link":"{url}","downloadPage":"{pageUrl}","suggestedName":"{fileName}"}],
                "options":{"silentAdd":true,"silentStart":true}
            }));
            let migrated=serde_json::to_value(&config).unwrap(); config.migrate_legacy_ab();
            assert_eq!(serde_json::to_value(&config).unwrap(),migrated);
        }
    }
    #[test] fn legacy_ab_migration_preserves_custom_configuration() {
        let endpoint="http://127.0.0.1:15151/start-headless-download";
        let template=r#"{"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":true}"#;
        for (name,endpoint,template) in [
            ("AB Download Manager (fixture)",endpoint,template),
            ("ab download manager",endpoint,template),
            ("AB Download Manager","http://127.0.0.1:15152/start-headless-download",template),
            ("AB Download Manager","http://localhost:15151/start-headless-download",template),
            ("AB Download Manager",endpoint,r#"{"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":false}"#),
            ("AB Download Manager",endpoint,r#"{"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":true,"fixtureOption":true}"#),
        ] {
            let mut config=ExternalDownloader { name:name.into(),endpoint:endpoint.into(),request_template:template.into() };
            let original=serde_json::to_value(&config).unwrap(); config.migrate_legacy_ab();
            assert_eq!(serde_json::to_value(&config).unwrap(),original,"{name} {endpoint} {template}");
        }
    }
    #[test] fn manual_checks_and_bounded_intervals_are_valid() {
        let mut settings=Settings::default();
        for minutes in [0,5,60,10080] {settings.check_interval_minutes=minutes;assert!(settings.validate().is_ok(),"{minutes}");}
        for minutes in [1,4,10081,u32::MAX] {settings.check_interval_minutes=minutes;assert!(settings.validate().is_err(),"{minutes}");}
    }
    #[test] fn old_settings_gain_startup_navigation_and_proxy_defaults() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        for key in ["startupPage", "startupGithubTab", "navigationOrder", "proxy", "libraryLayout", "downloadConnections", "useExternalDownloader", "externalDownloader"] { value.as_object_mut().unwrap().remove(key); }
        let settings: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.startup_page, "home"); assert_eq!(settings.startup_github_tab, "explore"); assert_eq!(settings.library_layout, "three");
        assert_eq!(settings.download_connections, 4); assert!(!settings.use_external_downloader); assert_eq!(settings.external_downloader.endpoint, "http://127.0.0.1:15151/add");
        for connections in [0,9,usize::MAX] { let mut invalid=settings.clone(); invalid.download_connections=connections; assert!(invalid.validate().is_err()); }
        assert_eq!(settings.navigation_order, default_navigation_order()); assert_eq!(settings.proxy.mode, "system");
        settings.validate().unwrap();
    }
    #[test] fn old_custom_navigation_preserves_order_and_defaults_sort() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["navigationOrder"] = serde_json::json!(["agent","github","library","updates","downloads","home"]);
        value.as_object_mut().unwrap().remove("librarySort");
        let mut s: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(s.navigation_order, ["agent","github","trainers","library","updates","downloads","home"]);
        assert_eq!(s.library_sort, "added"); s.validate().unwrap();
        s.library_sort = "invalid".into(); assert!(s.validate().is_err());
    }
    #[test] fn startup_navigation_and_note_limits_are_validated() {
        let mut settings = Settings::default(); settings.startup_page = "github".into(); settings.startup_github_tab = "feed".into();
        settings.navigation_order.reverse(); settings.validate().unwrap();
        settings.navigation_order[0] = settings.navigation_order[1].clone(); assert!(settings.validate().is_err());
        let note = RepositoryNote { repository: "  Owner/Repo  ".into(), name: "追".repeat(80), description: "新".repeat(240), updated_at: now() }.normalized().unwrap();
        assert_eq!(note.repository, "owner/repo");
        let mut invalid = note.clone(); invalid.name.push('超'); assert!(invalid.normalized().is_err());
        let mut invalid = note.clone(); invalid.description.push('超'); assert!(invalid.normalized().is_err());
        let mut invalid = note; invalid.name = "第一行\n第二行".into(); assert!(invalid.normalized().is_err());
    }
    #[test] fn optional_updates_preserve_omission_and_allow_explicit_clear() {
        let absent:ProjectChanges=serde_json::from_value(serde_json::json!({})).unwrap();assert!(absent.installed_version.is_none());
        let cleared:ProjectChanges=serde_json::from_value(serde_json::json!({"installedVersion":null})).unwrap();assert_eq!(cleared.installed_version,Some(None));
        let value=serde_json::to_value(test_project("owner","repo")).unwrap();assert!(value.get("installedVersion").is_some());assert!(value.get("installed_version").is_none());
    }
}
pub fn bounded_text(value: &str, max: usize, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.chars().any(|c| c == '\0') {
        return Err(format!("{}为空或过长", label));
    }
    Ok(value.to_owned())
}
