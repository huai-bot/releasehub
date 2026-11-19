use super::{models::*, windows_probe::{self, InstallerProcess, ProcessExit}, Core};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ProjectGuard {
    version: Option<String>, path: Option<String>, executable: Option<String>, source: Option<String>,
}
impl From<&Project> for ProjectGuard {
    fn from(p: &Project) -> Self {
        Self { version: p.installed_version.clone(), path: p.installed_path.clone(),
            executable: p.executable_path.clone(), source: p.installed_source.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Identity {
    Executable(String),
    Registry { hive_key: String, name: String, publisher: String, anchor: RegistryAnchor },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RegistryAnchor { Repository, Location(String), Executable(String) }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Observation {
    pub identity: Identity,
    pub version: Option<String>,
}
impl Identity {
    fn source(&self) -> &'static str {
        match self { Self::Executable(_) => "executable", Self::Registry { .. } => "registry" }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Probe {
    // A failed read is not proof that an earlier installation was absent.
    pub registry_complete: bool,
    pub executable_read: bool,
    pub observations: Vec<Observation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CheckRecord {
    pub check: InstallationCheck,
    pub guard: ProjectGuard,
    pub baseline: Probe,
    pub launched: bool,
    pub exit_pending: bool,
    pub reboot_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InstallBinding {
    pub project_id: String,
    pub guard: ProjectGuard,
    pub identities: Vec<Identity>,
    pub checked_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Version { parts: [u64; 4], pre: semver::Prerelease, build: semver::BuildMetadata }
fn version(s: &str) -> Option<Version> {
    let s = s.trim();
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
    let (s, build) = match s.split_once('+') {
        Some((s, b)) if !b.is_empty() => (s, semver::BuildMetadata::new(b).ok()?),
        Some(_) => return None,
        None => (s, semver::BuildMetadata::EMPTY),
    };
    let (s, pre) = match s.split_once('-') {
        Some((s, p)) if !p.is_empty() => (s, semver::Prerelease::new(p).ok()?),
        Some(_) => return None,
        None => (s, semver::Prerelease::EMPTY),
    };
    let mut parts = [0; 4];
    for (i, part) in s.split('.').enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
        *parts.get_mut(i)? = part.parse().ok()?;
    }
    Some(Version { parts, pre, build })
}
pub(super) fn recognizable_version(value: &str) -> bool { version(value).is_some() }
fn newer(candidate: &str, current: &str) -> bool {
    match (version(candidate), version(current)) {
        (Some(a), Some(b)) => a.parts.cmp(&b.parts).then_with(|| a.pre.cmp(&b.pre)) == Ordering::Greater,
        _ => false,
    }
}

const FAST_SECONDS: i64 = 120;
const DEADLINE_SECONDS: i64 = 15 * 60;
const SYNC_SECONDS: i64 = 30 * 60;
fn elapsed(now: DateTime<Utc>, value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value).map_or(i64::MAX, |at| now.signed_duration_since(at).num_seconds().max(0))
}
fn due(task: &CheckRecord, time: DateTime<Utc>, force: bool, boot_id: Option<&str>, exit_trigger: bool) -> bool {
    use InstallationStatus::*;
    if !task.launched || task.exit_pending || !task.check.status.can_check() { return false; }
    if task.check.status == RebootRequired && !matches!((task.reboot_id.as_deref(), boot_id), (Some(a), Some(b)) if a != b) {
        return false;
    }
    if task.check.status == Unconfirmed && !force { return false; }
    let age = elapsed(time, &task.check.started_at);
    let interval = if force { 5 } else if age < FAST_SECONDS { 5 } else { 15 };
    exit_trigger || task.check.checked_at.as_ref().is_none_or(|v| elapsed(time, v) >= interval)
}

// A unique, coherent observation must match the exact requested version and have changed
// since before launch. One registry app plus its independent selected executable can agree.
fn coherent(probe: &Probe) -> Option<(&Observation, Vec<Identity>)> {
    let mut executable = None;
    let mut registry = None;
    for observation in &probe.observations {
        let slot = match observation.identity { Identity::Executable(_) => &mut executable, Identity::Registry { .. } => &mut registry };
        if slot.replace(observation).is_some() { return None; }
    }
    if registry.is_some() && !probe.registry_complete { return None; }
    if let (Some(a), Some(b)) = (executable, registry) {
        if let (Some(a), Some(b)) = (a.version.as_deref().and_then(version), b.version.as_deref().and_then(version)) {
            if a != b { return None; }
        }
    }
    let primary = executable.filter(|o| o.version.as_deref().and_then(version).is_some()).or(registry)?;
    if primary.version.as_deref().and_then(version).is_none() { return None; }
    Some((primary, executable.into_iter().chain(registry).map(|o| o.identity.clone()).collect()))
}
fn exact_evidence(task: &CheckRecord, probe: &Probe) -> Option<(&'static str, String, Vec<Identity>)> {
    let (observation, identities) = coherent(probe)?;
    let expected = version(&task.check.expected_version)?;
    if observation.version.as_deref().and_then(version)? != expected { return None; }
    // Don't turn an already present version into a newly successful install, even if the
    // installer swapped the uninstall key or wrote a new evidence source for that release.
    if task.baseline.observations.iter().any(|before| before.version.as_deref().and_then(version).as_ref() == Some(&expected)) { return None; }
    let baseline_read = match observation.identity {
        Identity::Executable(_) => task.baseline.executable_read,
        Identity::Registry { .. } => task.baseline.registry_complete,
    };
    if !baseline_read { return None; }
    if task.baseline.observations.iter().filter(|before| before.identity.source() == observation.identity.source()).count() > 1 { return None; }
    // Previously unreadable version metadata by itself is not proof of an upgrade.
    if task.baseline.observations.iter().any(|before| before.identity == observation.identity
        && before.version.as_deref().and_then(version).is_none()) { return None; }
    Some((observation.identity.source(), observation.version.clone()?, identities))
}

fn record_observation(mut task: CheckRecord, probe: &Probe, time: DateTime<Utc>) -> (CheckRecord, Option<Vec<Identity>>) {
    let evidence = exact_evidence(&task, probe);
    task.check.checked_at = Some(time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    if let Some((source, version, identities)) = evidence {
        task.check.status = InstallationStatus::Verified;
        task.check.source = Some(source.into());
        task.check.detected_version = Some(version);
        task.check.message = "已检测到本次安装的版本。".into();
        return (task, Some(identities));
    }
    let expired = elapsed(time, &task.check.started_at) >= DEADLINE_SECONDS;
    let seen = coherent(probe).and_then(|(o, _)| o.version.clone().map(|v| (v, o.identity.source())));
    task.check.detected_version = seen.as_ref().map(|(version, _)| version.clone());
    task.check.source = seen.map(|(_, source)| source.into());
    task.check.status = if expired { InstallationStatus::Unconfirmed } else { InstallationStatus::Detecting };
    task.check.message = if expired { "未检测到本次安装的版本。安装确已完成时，可手动确认。" }
        else { "正在检查本机版本；如果安装向导仍在运行，请继续完成。" }.into();
    (task, None)
}

impl Core {
    pub(super) fn prepare_installation_check(&self, p: &Project, d: &DownloadRecord, baseline: Probe) -> CheckRecord {
        CheckRecord {
            check: InstallationCheck { id: uuid::Uuid::new_v4().to_string(), project_id: p.id.clone(), download_id: d.id.clone(),
                expected_version: d.tag.clone(), previous_version: p.installed_version.clone(), status: InstallationStatus::Waiting,
                started_at: now(), checked_at: None, detected_version: None, source: None, message: "等待官方安装向导完成。".into() },
            guard: p.into(), baseline, launched: false, exit_pending: false, reboot_id: None,
        }
    }

    pub async fn verify_installations(self: &Arc<Self>, force: bool) -> Result<Vec<InstallationCheck>, String> {
        self.verify_installations_for(None, force, None).await
    }
    pub(super) async fn verify_installations_for(self: &Arc<Self>, project_id: Option<String>, force: bool, exited: Option<String>) -> Result<Vec<InstallationCheck>, String> {
        let core = self.clone();
        // Explicit checks wait for the current read; periodic ticks skip overlap.
        let _guard = if force { self.installation_verification.lock().await } else {
            match self.installation_verification.try_lock() {
                Ok(guard) => guard,
                Err(_) => return self.store.installations(project_id.as_deref()),
            }
        };
        tokio::task::spawn_blocking(move || core.verify_installations_with(project_id.as_deref(), force, exited.as_deref(),
            Utc::now(), windows_probe::boot_id().as_deref(), windows_probe::probe))
            .await.map_err(|_| "安装验证任务被中断".to_string())?
    }
    pub(super) fn verify_installations_with(&self, project_id: Option<&str>, force: bool, exited: Option<&str>,
        time: DateTime<Utc>, boot_id: Option<&str>, mut probe: impl FnMut(&Project, &[Identity]) -> Probe) -> Result<Vec<InstallationCheck>, String> {
        let mut changed = false;
        let records = self.store.active_installation_records(project_id)?;
        let active: std::collections::HashSet<_> = records.iter().map(|r| r.check.project_id.clone()).collect();
        for task in records {
            let id = &task.check.project_id;
            if task.launched && task.exit_pending && task.check.status == InstallationStatus::Waiting
                && elapsed(time, &task.check.started_at) >= DEADLINE_SECONDS {
                let mut next = task.clone();
                next.check.status = InstallationStatus::Unconfirmed;
                next.check.message = "安装向导仍未退出，尚不能核实版本。请完成安装向导。".into();
                changed |= self.store.publish_installation_probe(&task, &next, None)?.0;
                continue;
            }
            if !due(&task, time, force, boot_id, exited == Some(task.check.id.as_str())) { continue; }
            let Ok(p) = self.store.project(id) else { continue; };
            if p.management_mode != "software" || ProjectGuard::from(&p) != task.guard { continue; }
            let binding = self.store.install_binding(id)?;
            let identities = binding.as_ref().map_or(&[][..], |b| b.identities.as_slice());
            let observed = probe(&p, identities);
            let (next, binding) = record_observation(task.clone(), &observed, time);
            let installing = self.installing.lock().map_err(|_| "安装状态不可用".to_string())?;
            if installing.contains(id) { continue; }
            let (written, download) = self.store.publish_installation_probe(&task, &next, binding)?;
            drop(installing);
            if let Some(ref download) = download { self.download_event(download); }
            changed |= written && (download.is_some() || task.check.status != next.check.status
                || task.check.detected_version != next.check.detected_version || task.check.source != next.check.source);
        }
        for binding in self.store.install_bindings(project_id)? {
            let id = binding.project_id.as_str();
            if active.contains(id) || elapsed(time, &binding.checked_at) < SYNC_SECONDS { continue; }
            let Ok(p) = self.store.project(id) else { continue; };
            if p.management_mode != "software" || p.installed_source.as_deref() != Some("installer") || ProjectGuard::from(&p) != binding.guard { continue; }
            let observed = probe(&p, &binding.identities);
            // Don't add new identities or ignore competing matches during background sync.
            let detected = coherent(&observed).filter(|(o, identities)| binding.identities.contains(&o.identity)
                && identities.iter().all(|identity| binding.identities.contains(identity))).and_then(|(o, _)| o.version.as_deref())
                .filter(|candidate| p.installed_version.as_deref().is_some_and(|current| newer(candidate, current)));
            let installing = self.installing.lock().map_err(|_| "安装状态不可用".to_string())?;
            if installing.contains(id) { continue; }
            changed |= self.store.sync_install_binding(&binding, detected, time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))?;
        }
        if changed { self.changed(); }
        self.store.installations(project_id)
    }

    pub(super) fn confirm_installation(&self, id: &str) -> Result<Project, String> {
        let installing = self.installing.lock().map_err(|_| "安装状态不可用".to_string())?;
        let task = self.store.installation_record(id)?;
        if installing.contains(&task.check.project_id) { return Err("正在启动安装或恢复，请稍后再试".into()); }
        let (project, download) = self.store.confirm_installation(&task)?;
        drop(installing);
        self.download_event(&download);
        self.changed();
        Ok(project)
    }

    pub(super) fn watch_installer(self: &Arc<Self>, id: String, project_id: String, process: InstallerProcess) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let Some(core) = weak.upgrade() else { return; };
                let Ok(task) = core.store.installation_record(&id) else { return; };
                if !task.check.status.can_check() || !task.exit_pending { return; }
                let exit = process.exit();
                if exit == ProcessExit::Running { continue; }
                let result = core.store.record_installer_exit(&id, exit, windows_probe::boot_id());
                if let Ok(true) = result {
                    core.changed();
                    if matches!(exit, ProcessExit::Code(0) | ProcessExit::Unavailable) {
                        let _ = core.verify_installations_for(Some(project_id), true, Some(id)).await;
                    }
                }
                return;
            }
        });
    }
}

#[cfg(test)]
#[path = "installation_checks_tests.rs"]
mod tests;
