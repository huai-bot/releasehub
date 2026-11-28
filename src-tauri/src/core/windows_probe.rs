use super::{installation_checks::{Identity, Probe}, models::Project};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProcessExit { Running, Code(u32), Unavailable }

pub(super) struct LaunchError { pub cancelled: bool, pub message: String }
impl LaunchError {
    fn new(message: &str, cancelled: bool) -> Self { Self { cancelled, message: message.into() } }
}

#[cfg(not(windows))]
pub(super) struct InstallerProcess;
#[cfg(not(windows))]
impl InstallerProcess { pub(super) fn exit(&self) -> ProcessExit { ProcessExit::Unavailable } }
#[cfg(not(windows))]
pub(super) fn probe(_: &Project, _: &[Identity]) -> Probe { Probe::default() }
#[cfg(not(windows))]
pub(super) fn boot_id() -> Option<String> { None }
#[cfg(not(windows))]
pub(super) fn launch(_: &std::path::Path) -> Result<Option<InstallerProcess>, LaunchError> {
    Err(LaunchError::new("安装器只能在 Windows 上启动", false))
}

#[derive(Default)]
pub(super) struct LocalAppMetadata {
    pub version: Option<String>,
    pub repository_urls: Vec<String>,
    pub product: Option<String>,
    pub publisher: Option<String>,
    pub original_filename: Option<String>,
}

#[cfg(not(windows))]
pub(super) fn local_executable_path(path: &std::path::Path) -> bool { path.is_absolute() }
#[cfg(not(windows))]
pub(super) fn local_app_metadata(_: &std::path::Path) -> LocalAppMetadata { LocalAppMetadata::default() }
#[cfg(not(windows))]
pub(super) fn local_app_version(_: &std::path::Path) -> Option<String> { None }

#[cfg(windows)]
mod native {
    use super::*;
    use crate::core::installation_checks::{recognizable_version, Observation, RegistryAnchor};
    use std::{collections::HashMap, ffi::c_void, io, mem::{size_of, zeroed},
        os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle, OwnedHandle}},
        path::{Component, Path, PathBuf, Prefix}, ptr::null_mut};
    use winreg::{enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY}, RegKey};

    #[repr(C)]
    struct ShellExecuteInfo {
        size: u32, mask: u32, hwnd: *mut c_void, verb: *const u16, file: *const u16,
        parameters: *const u16, directory: *const u16, show: i32, instance: *mut c_void,
        id_list: *mut c_void, class: *const u16, class_key: *mut c_void, hot_key: u32,
        icon_or_monitor: *mut c_void, process: *mut c_void,
    }
    #[link(name = "shell32")]
    extern "system" { fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32; }
    #[link(name = "ole32")]
    extern "system" { fn CoInitializeEx(reserved: *mut c_void, concurrency: u32) -> i32; fn CoUninitialize(); }
    #[link(name = "kernel32")]
    extern "system" {
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    #[link(name = "version")]
    extern "system" {
        fn GetFileVersionInfoSizeExW(flags: u32, filename: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoExW(flags: u32, filename: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(data: *const c_void, sub_block: *const u16, value: *mut *mut c_void, len: *mut u32) -> i32;
    }
    #[link(name = "ntdll")]
    extern "system" { fn NtQuerySystemInformation(class: u32, data: *mut c_void, len: u32, out: *mut u32) -> i32; }

    pub(in crate::core) struct InstallerProcess(OwnedHandle);
    impl InstallerProcess {
        pub(in crate::core) fn exit(&self) -> ProcessExit {
            let h = self.0.as_raw_handle();
            match unsafe { WaitForSingleObject(h, 0) } {
                0 => { let mut code = 0; if unsafe { GetExitCodeProcess(h, &mut code) } != 0 { ProcessExit::Code(code) } else { ProcessExit::Unavailable } }
                258 => ProcessExit::Running,
                _ => ProcessExit::Unavailable,
            }
        }
    }
    pub(in crate::core) fn boot_id() -> Option<String> {
        // SystemBootEnvironmentInformation begins with the current boot GUID.
        // If unavailable, leave reboot-required tasks untouched rather than guessing.
        let mut data = [0u64; 8];
        let mut len = 0;
        let status = unsafe { NtQuerySystemInformation(90, data.as_mut_ptr().cast(), size_of::<[u64; 8]>() as u32, &mut len) };
        (status >= 0 && len >= 16 && (data[0] != 0 || data[1] != 0)).then(|| format!("{:016x}{:016x}", data[0], data[1]))
    }
    pub(in crate::core) fn launch(path: &Path) -> Result<Option<InstallerProcess>, LaunchError> {
        if !path.extension().and_then(|s| s.to_str()).is_some_and(|e| ["exe", "msi"].iter().any(|ext| e.eq_ignore_ascii_case(ext))) {
            return Err(LaunchError::new("仅支持 EXE/MSI 安装器", false));
        }
        let file = wide(path.as_os_str());
        let verb = wide("open");
        // Shell extensions may need a single-threaded COM apartment. Only uninitialize
        // an apartment we successfully initialized (and do it on this same thread).
        struct Com;
        impl Drop for Com { fn drop(&mut self) { unsafe { CoUninitialize() }; } }
        let init = unsafe { CoInitializeEx(null_mut(), 0x2 | 0x4) };
        let _com = if init >= 0 { Some(Com) } else if init == 0x80010106u32 as i32 { None }
            else { return Err(LaunchError::new("无法初始化 Windows 安装向导", false)); };
        let mut info: ShellExecuteInfo = unsafe { zeroed() };
        info.size = size_of::<ShellExecuteInfo>() as u32;
        // NOCLOSEPROCESS | NOASYNC | FLAG_NO_UI. The app never adds runas or extra arguments.
        info.mask = 0x40 | 0x100 | 0x400;
        info.verb = verb.as_ptr(); info.file = file.as_ptr(); info.show = 1;
        let opened = unsafe { ShellExecuteExW(&mut info) };
        let error = (opened == 0).then(io::Error::last_os_error);
        let process = if info.process.is_null() { None } else { Some(InstallerProcess(unsafe { OwnedHandle::from_raw_handle(info.process) })) };
        if let Some(error) = error {
            if error.raw_os_error() == Some(1223) { return Err(LaunchError::new("已取消启动安装向导。", true)); }
            return Err(LaunchError::new("安装向导未能启动，请检查安装包后重试。", false));
        }
        Ok(process)
    }

    fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> { value.as_ref().encode_wide().chain(Some(0)).collect() }
    fn local_path(value: &str) -> Option<(PathBuf, String)> {
        let value = value.trim();
        let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
        if value.len() > 4096 || value.chars().any(|c| c.is_control() || matches!(c, '"' | '*' | '?' | '<' | '>' | '|')) {
            // Windows canonical paths have the verbatim prefix, which is not itself a wildcard.
            if !value.starts_with(r"\\?\") || value[4..].chars().any(|c| c.is_control() || matches!(c, '"' | '*' | '?' | '<' | '>' | '|')) { return None; }
        }
        let value = value.strip_prefix(r"\\?\").unwrap_or(value).replace('/', "\\");
        let path = PathBuf::from(value);
        let mut components = path.components();
        let Component::Prefix(prefix) = components.next()? else { return None; };
        let Prefix::Disk(drive) = prefix.kind() else { return None; };
        if components.next()? != Component::RootDir { return None; }
        let mut normalized = format!("{}:\\", drive as char);
        for component in components {
            let Component::Normal(part) = component else { return None; };
            let part = part.to_str()?;
            if part.contains(':') || part.ends_with(['.', ' ']) { return None; }
            if !normalized.ends_with('\\') { normalized.push('\\'); }
            normalized.push_str(part);
        }
        // A volume root is not a strong application-location identity.
        if normalized.len() <= 3 { return None; }
        Some((path, normalized.to_lowercase()))
    }
    fn fixed_drive(path: &str) -> bool { unsafe { GetDriveTypeW(wide(&path[..3]).as_ptr()) == 3 } }
    fn display_icon(s: &str) -> Option<String> {
        let s = s.trim();
        let path = if let Some(quoted) = s.strip_prefix('"') {
            let (path, rest) = quoted.split_once('"')?;
            let rest = rest.trim();
            if !rest.is_empty() && rest.strip_prefix(',').and_then(|n| n.trim().parse::<i32>().ok()).is_none() { return None; }
            path
        } else { s.rsplit_once(',').filter(|(_, n)| n.trim().parse::<i32>().is_ok()).map_or(s, |(path, _)| path.trim()) };
        let (p, identity) = local_path(path)?;
        p.extension().and_then(|s| s.to_str()).filter(|e| e.eq_ignore_ascii_case("exe"))?;
        Some(identity)
    }
    fn repository_url(s: &str, p: &Project) -> bool {
        let Ok(u) = url::Url::parse(s.trim()) else { return false; };
        if u.scheme() != "https" || u.host_str() != Some("github.com") || u.port().is_some() || !u.username().is_empty() || u.password().is_some() { return false; }
        let Some(mut path) = u.path_segments() else { return false; };
        let (Some(owner), Some(repo)) = (path.next(), path.next()) else { return false; };
        owner.eq_ignore_ascii_case(&p.owner) && repo.strip_suffix(".git").unwrap_or(repo).eq_ignore_ascii_case(&p.repo)
    }
    fn first_anchor(matches_name: bool, repo: bool, icon: Option<String>, location: Option<String>,
        saved_exe: Option<&String>, saved_dir: Option<&String>) -> Option<RegistryAnchor> {
        if repo { Some(RegistryAnchor::Repository) }
        else if matches_name && icon.is_some() && icon.as_ref() == saved_exe { icon.map(RegistryAnchor::Executable) }
        else if matches_name && location.is_some() && location.as_ref() == saved_dir { location.map(RegistryAnchor::Location) }
        else { None }
    }
    fn field(sub: &RegKey, name: &str, max: usize) -> String {
        sub.get_value::<String, _>(name).ok().map(|s| s.trim().to_owned())
            .filter(|s| s.len() <= max && !s.chars().any(char::is_control)).unwrap_or_default()
    }
    fn registry(p: &Project, result: &mut Probe, bindings: &[Identity]) {
        const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
        let mut observed = HashMap::<String, Vec<Observation>>::new();
        let mut complete = true;
        let saved_dir = p.installed_path.as_deref().and_then(local_path).map(|(_, id)| id);
        let saved_exe = p.executable_path.as_deref().and_then(display_icon);
        for (hive, prefix) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                let uninstall = match RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | view) {
                    Ok(reg) => reg,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(_) => { complete = false; continue; }
                };
                for key in uninstall.enum_keys() {
                    let key = match key { Ok(key) => key, Err(_) => { complete = false; continue; } };
                    let entry = match uninstall.open_subkey_with_flags(&key, KEY_READ | view) {
                        Ok(entry) => entry,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(_) => { complete = false; continue; }
                    };
                    let name = field(&entry, "DisplayName", 512).to_lowercase();
                    if name.is_empty() { continue; }
                    let publisher = field(&entry, "Publisher", 512).to_lowercase();
                    let matches_name = name.eq_ignore_ascii_case(&p.name.to_lowercase()) || name.eq_ignore_ascii_case(&p.repo.to_lowercase());
                    let repo = ["URLInfoAbout", "HelpLink", "URLUpdateInfo"].iter().any(|k| repository_url(&field(&entry, k, 2048), p));
                    let icon = display_icon(&field(&entry, "DisplayIcon", 4096));
                    let location = local_path(&field(&entry, "InstallLocation", 4096)).map(|(_, id)| id);
                    let key_id = format!("{prefix}\\{}", key.to_lowercase());
                    let preferred = bindings.iter().find_map(|id| match id {
                        Identity::Registry { hive_key, name: bound_name, publisher: bound_publisher, anchor }
                            if hive_key == &key_id && bound_name == &name && bound_publisher == &publisher => match anchor {
                                RegistryAnchor::Repository if repo => Some(anchor.clone()),
                                RegistryAnchor::Executable(bound) if icon.as_ref() == Some(bound) && saved_exe.as_ref() == Some(bound) => Some(anchor.clone()),
                                RegistryAnchor::Location(bound) if location.as_ref() == Some(bound) && saved_dir.as_ref() == Some(bound) => Some(anchor.clone()),
                                _ => None,
                            },
                        _ => None,
                    });
                    let anchor = preferred.or_else(|| first_anchor(matches_name, repo, icon, location, saved_exe.as_ref(), saved_dir.as_ref()));
                    if let Some(anchor) = anchor {
                        let version = field(&entry, "DisplayVersion", 200);
                        let new = Observation { identity: Identity::Registry { hive_key: key_id.clone(), name, publisher, anchor },
                            version: recognizable_version(&version).then_some(version) };
                        let values = observed.entry(key_id).or_default();
                        // HKCU can expose the same physical uninstall key in both views.
                        // Separate installations or contradictory values remain ambiguous.
                        if !values.contains(&new) || prefix == "HKLM" { values.push(new); }
                    }
                }
            }
        }
        result.registry_complete = complete;
        result.observations.extend(observed.into_values().flatten());
    }

    fn query(data: &[u8], block: &str, width: usize) -> Option<(*const u8, usize)> {
        let mut value = null_mut(); let mut len = 0;
        if unsafe { VerQueryValueW(data.as_ptr().cast(), wide(block).as_ptr(), &mut value, &mut len) } == 0 || value.is_null() { return None; }
        let start = (value as usize).checked_sub(data.as_ptr() as usize)?;
        let bytes = (len as usize).checked_mul(width)?;
        if start.checked_add(bytes)? > data.len() { return None; }
        Some((value.cast(), bytes))
    }
    fn version_string(data: &[u8], translation: u32, field: &str) -> Option<String> {
        let block = format!("\\StringFileInfo\\{translation:08x}\\{field}");
        let (ptr, bytes) = query(data, &block, 2)?;
        if !(4..=402).contains(&bytes) { return None; }
        let chars = (0..bytes/2).map(|i| unsafe { ptr.add(i*2).cast::<u16>().read_unaligned() }).take_while(|c| *c != 0).collect::<Vec<_>>();
        let value = String::from_utf16(&chars).ok()?.trim().to_owned();
        (!value.is_empty()).then_some(value)
    }
    fn version_resource(path: &Path) -> Option<(Vec<u8>, Vec<u32>)> {
        let filename = wide(path.as_os_str());
        let mut ignored = 0;
        const NEUTRAL: u32 = 2;
        let size = unsafe { GetFileVersionInfoSizeExW(NEUTRAL, filename.as_ptr(), &mut ignored) };
        if !(52..=1_048_576).contains(&size) { return None; }
        let mut data = vec![0; size as usize];
        if unsafe { GetFileVersionInfoExW(NEUTRAL, filename.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 { return None; }
        let mut translations = Vec::new();
        if let Some((ptr, bytes)) = query(&data, "\\VarFileInfo\\Translation", 1) {
            for i in (0..bytes / 4).take(64) {
                let pair = unsafe { ptr.add(i*4).cast::<u32>().read_unaligned() };
                translations.push(pair.rotate_left(16));
            }
        }
        for translation in [0x040904b0, 0x040904e4, 0x000004b0] {
            if !translations.contains(&translation) { translations.push(translation); }
        }
        Some((data, translations))
    }
    fn version_from_resource(data: &[u8], translations: &[u32], normalize: bool) -> Option<String> {
        let (ptr, len) = query(data, "\\", 1)?;
        if len < 52 { return None; }
        let word = |i: usize| unsafe { ptr.add(i * 4).cast::<u32>().read_unaligned() };
        if word(0) != 0xFEEF04BD { return None; }
        let flags = word(6) & word(7);
        let mut values = Vec::new();
        for field in ["ProductVersion", "FileVersion"] {
            let mut found = Vec::new();
            for &trans in translations {
                if let Some(s) = version_string(data, trans, field) {
                    if !found.contains(&s) { found.push(s); }
                }
            }
            if found.len() > 1 { return None; }
            values.extend(found);
        }
        let mut prereleases = values.iter().filter(|s| s.split('+').next().is_some_and(|v| v.contains('-')));
        if let Some(pre) = prereleases.next() {
            if prereleases.any(|other| other != pre) { return None; }
            return if normalize { normalize_local_version(pre) } else { recognizable_version(pre).then(|| pre.clone()) };
        }
        if flags & (0x1 | 0x2 | 0x8 | 0x20) != 0 { return None; }
        if let Some(v) = values.first() {
            return if normalize { normalize_local_version(v) } else { recognizable_version(v).then(|| v.clone()) };
        }
        let (mut ms, mut ls) = (word(4), word(5));
        if ms == 0 && ls == 0 { (ms, ls) = (word(2), word(3)); }
        if ms == 0 && ls == 0 { return None; }
        let fixed = format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff);
        if normalize { normalize_local_version(&fixed) } else { Some(fixed) }
    }
    fn file_version(path: &Path) -> Option<String> {
        let (data, translations) = version_resource(path)?;
        version_from_resource(&data, &translations, false)
    }
    fn normalize_local_version(value: &str) -> Option<String> {
        use crate::core::selection::version;
        if version(value).is_some() { return Some(value.into()); }
        // Win32 often supplies 1.2.3.0 when the release is 1.2.3. A nonzero revision is not equivalent.
        let (numeric, suffix) = value.split_at(value.find(['-', '+']).unwrap_or(value.len()));
        let (base, revision) = numeric.rsplit_once('.')?;
        if base.split('.').count() != 3 || revision != "0" { return None; }
        let normalized = format!("{base}{suffix}");
        version(&normalized).map(|_| normalized)
    }
    #[cfg(test)]
    #[test]
    fn local_resource_version_preserves_prereleases_and_only_discards_zero_revision() {
        assert_eq!(normalize_local_version("1.2.3.0").as_deref(), Some("1.2.3"));
        assert_eq!(normalize_local_version("1.2.3.0-beta.1").as_deref(), Some("1.2.3-beta.1"));
        assert_eq!(normalize_local_version("1.2.3.7"), None);
        assert_eq!(normalize_local_version("nightly"), None);
    }
    fn unique_version_field(data: &[u8], translations: &[u32], field: &str) -> Option<String> {
        let mut fields = translations.iter().filter_map(|&t| version_string(data, t, field));
        let first = fields.next()?;
        (!fields.any(|value| !first.eq_ignore_ascii_case(&value))).then_some(first)
    }
    pub(in crate::core) fn local_executable_path(path: &Path) -> bool {
        local_path(path.to_str().unwrap_or_default()).is_some_and(|(_, id)| {
            // Include removable disks for portable EXEs; exclude mapped network drives.
            matches!(unsafe { GetDriveTypeW(wide(&id[..3]).as_ptr()) }, 2 | 3)
        })
    }
    pub(in crate::core) fn local_app_version(path: &Path) -> Option<String> {
        let (data, translations) = version_resource(path)?;
        version_from_resource(&data, &translations, true)
    }
    pub(in crate::core) fn local_app_metadata(path: &Path) -> LocalAppMetadata {
        let mut result = LocalAppMetadata::default();
        if let Some((data, translations)) = version_resource(path) {
            result.version = version_from_resource(&data, &translations, true);
            result.product = unique_version_field(&data, &translations, "ProductName");
            result.publisher = unique_version_field(&data, &translations, "CompanyName");
            result.original_filename = unique_version_field(&data, &translations, "OriginalFilename");
            for field in ["RepositoryUrl", "RepositoryURL", "ProjectUrl", "SourceUrl", "Homepage", "Website", "Comments"] {
                for &t in &translations {
                    if let Some(hint) = version_string(&data, t, field) {
                        if !result.repository_urls.contains(&hint) { result.repository_urls.push(hint); }
                    }
                }
            }
        }
        if let Some((_, identity)) = path.to_str().and_then(local_path) {
            collect_uninstall_repository_urls(&identity, &mut result.repository_urls);
        }
        result
    }
    fn collect_uninstall_repository_urls(exe: &str, urls: &mut Vec<String>) {
        const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                let Ok(uninstall) = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | view) else { continue; };
                for key in uninstall.enum_keys().take(4096).flatten() {
                    let Ok(entry) = uninstall.open_subkey_with_flags(key, KEY_READ | view) else { continue; };
                    if display_icon(&field(&entry, "DisplayIcon", 4096)).as_deref() != Some(exe) { continue; }
                    for key in ["URLInfoAbout", "URLUpdateInfo"] {
                        let url = field(&entry, key, 2048);
                        if !url.is_empty() && !urls.contains(&url) { urls.push(url); }
                    }
                }
            }
        }
    }
    #[cfg(test)]
    #[test]
    fn registry_identity_requires_the_exact_repository_or_a_name_and_exact_path() {
        let p = crate::core::models::test_project("fixture", "App");
        assert!(repository_url("https://github.com/Fixture/App/releases", &p));
        assert!(!repository_url("https://github.com/fixture/App-helper", &p));
        assert!(!repository_url("https://github.com.example/fixture/app", &p));
        assert!(!repository_url("https://user@github.com/fixture/app", &p));
        let executable = display_icon(r#""C:\Apps\Tool\app.exe",0"#).unwrap();
        assert_eq!(first_anchor(true, false, None, None, Some(&executable), None), None);
        assert_eq!(first_anchor(false, false, Some(executable.clone()), None, Some(&executable), None), None);
        assert_eq!(first_anchor(true, false, display_icon(r"C:\Other\Tool\app.exe"), None, Some(&executable), None), None);
        assert_eq!(first_anchor(true, false, Some(executable.clone()), None, Some(&executable), None), Some(RegistryAnchor::Executable(executable.clone())));
        assert_eq!(first_anchor(false, true, None, None, None, None), Some(RegistryAnchor::Repository));
        assert!(display_icon(r"\\server\app.exe").is_none());
    }
    pub(in crate::core) fn probe(p: &Project, bindings: &[Identity]) -> Probe {
        let mut result = Probe::default();
        if let Some((path, id)) = p.executable_path.as_deref().and_then(local_path)
            .filter(|(path, _)| path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe"))) {
            if fixed_drive(&id) {
                match std::fs::canonicalize(&path) {
                    Ok(real) if local_path(&real.to_string_lossy()).is_some_and(|(_, canonical)| canonical == id)
                        && std::fs::metadata(&real).is_ok_and(|m| m.is_file()) => {
                            result.executable_read = true;
                            result.observations.push(Observation { identity: Identity::Executable(id), version: file_version(&real) });
                        }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => { result.executable_read = true; }
                    _ => {}
                }
            }
        }
        registry(p, &mut result, bindings);
        result
    }
}
#[cfg(windows)]
pub(super) use native::{boot_id, launch, local_app_metadata, local_app_version, local_executable_path, probe, InstallerProcess};
