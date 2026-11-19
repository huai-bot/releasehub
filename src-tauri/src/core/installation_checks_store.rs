use super::{params, storage_error, Connection, DownloadRecord, InstallJournal, OptionalExtension, Project, Store};
use crate::core::{installation_checks::{CheckRecord, Identity, InstallBinding, ProjectGuard}, models::{now, InstallationCheck, InstallationStatus}, selection, windows_probe::ProcessExit};
use serde::{de::DeserializeOwned, Serialize};
use InstallationStatus::*;

const ACTIVE: &str = "json_extract(json,'$.check.status') IN ('waiting','detecting','rebootRequired','unconfirmed')";
fn read<T: DeserializeOwned>(db: &Connection, table: &str, id: &str) -> Result<Option<T>, String> {
    let json: Option<String> = db.query_row(&format!("SELECT json FROM {table} WHERE id=?1"), [id], |r| r.get(0)).optional().map_err(storage_error)?;
    json.map(|value| serde_json::from_str(&value).map_err(storage_error)).transpose()
}
fn write(db: &Connection, table: &str, id: &str, value: &impl Serialize) -> Result<(), String> {
    db.execute(&format!("UPDATE {table} SET json=?1 WHERE id=?2"), params![serde_json::to_string(value).map_err(storage_error)?, id]).map_err(storage_error)?;
    Ok(())
}
fn list<T: DeserializeOwned>(db: &Connection, sql: &str, project: Option<&str>) -> Result<Vec<T>, String> {
    let mut stmt = db.prepare(sql).map_err(storage_error)?;
    let rows = stmt.query_map([project], |r| r.get::<_, String>(0)).map_err(storage_error)?;
    rows.map(|r| serde_json::from_str(&r.map_err(storage_error)?).map_err(storage_error)).collect()
}
fn supersede(db: &Connection, column: &str, id: &str, message: &str) -> Result<(), String> {
    db.execute(&format!("UPDATE installation_checks SET json=json_set(json,'$.check.status','superseded','$.check.message',?2)
        WHERE {column}=?1 AND {ACTIVE}"), params![id, message]).map_err(storage_error)?;
    Ok(())
}
pub(super) fn invalidate_project(db: &Connection, id: &str) -> Result<(), String> {
    supersede(db, "project_id", id, "本机安装记录已更改，已停止核实此任务。")?;
    db.execute("DELETE FROM installation_bindings WHERE id=?1", [id]).map_err(storage_error)?;
    Ok(())
}
pub(super) fn invalidate_download(db: &Connection, id: &str) -> Result<(), String> {
    supersede(db, "download_id", id, "原下载记录已删除，已停止核实此任务。")
}
fn compatible(task: &CheckRecord, p: &Project, d: &DownloadRecord) -> bool {
    p.management_mode == "software" && p.id == task.check.project_id && ProjectGuard::from(p) == task.guard
        && d.project_id == p.id && d.id == task.check.download_id && d.tag == task.check.expected_version
        && d.kind == "installer" && d.installable && matches!(d.status.as_str(), "completed" | "installed" | "installing")
}

impl Store {
    pub(in crate::core) fn installation_record(&self, id: &str) -> Result<CheckRecord, String> { self.get("installation_checks", id) }
    pub(in crate::core) fn installations(&self, project: Option<&str>) -> Result<Vec<InstallationCheck>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let records: Vec<CheckRecord> = list(&db, "SELECT json FROM installation_checks WHERE (?1 IS NULL OR project_id=?1) ORDER BY rowid DESC", project)?;
        Ok(records.into_iter().map(|r| r.check).collect())
    }
    pub(in crate::core) fn active_installation_records(&self, project: Option<&str>) -> Result<Vec<CheckRecord>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        list(&db, &format!("SELECT json FROM installation_checks WHERE (?1 IS NULL OR project_id=?1) AND {ACTIVE} ORDER BY rowid DESC"), project)
    }
    pub(in crate::core) fn install_bindings(&self, project: Option<&str>) -> Result<Vec<InstallBinding>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        list(&db, "SELECT json FROM installation_bindings WHERE (?1 IS NULL OR id=?1)", project)
    }
    pub(in crate::core) fn install_binding(&self, project: &str) -> Result<Option<InstallBinding>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        read(&db, "installation_bindings", project)
    }
    pub(in crate::core) fn begin_installation_check(&self, task: &CheckRecord, journal: Option<&InstallJournal>) -> Result<(), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let p = read(&tx, "projects", &task.check.project_id)?.ok_or("要安装的项目已删除")?;
        let d = read(&tx, "downloads", &task.check.download_id)?.ok_or("安装包已删除")?;
        if !compatible(task, &p, &d) { return Err("安装状态已改变，请重新确认".into()); }
        supersede(&tx, "project_id", &p.id, "已启动此项目的新安装任务。")?;
        // Keep completed history bounded per project; stale async work still cannot recreate a removed task.
        tx.execute("DELETE FROM installation_checks WHERE project_id=?1 AND id NOT IN (
            SELECT id FROM installation_checks WHERE project_id=?1 ORDER BY rowid DESC LIMIT 9)", [&p.id]).map_err(storage_error)?;
        tx.execute("INSERT INTO installation_checks(id,project_id,download_id,json) VALUES(?1,?2,?3,?4)",
            params![task.check.id, task.check.project_id, task.check.download_id, serde_json::to_string(task).map_err(storage_error)?]).map_err(storage_error)?;
        if let Some(j) = journal { write(&tx, "install_journals", &p.id, j)?; }
        tx.commit().map_err(storage_error)
    }
    pub(in crate::core) fn fail_installer_launch(&self, id: &str, cancelled: bool, message: &str) -> Result<(), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        if let Some(mut task) = read::<CheckRecord>(&tx, "installation_checks", id)? {
            if !task.launched && task.check.status == Waiting {
                task.check.status = if cancelled { Cancelled } else { Failed };
                task.check.message = message.into(); task.check.checked_at = Some(now());
                write(&tx, "installation_checks", id, &task)?;
            }
        }
        tx.commit().map_err(storage_error)
    }
    pub(in crate::core) fn finish_installer_launch(&self, id: &str, journal: Option<&InstallJournal>, exit_pending: bool,
        recovering: bool) -> Result<DownloadRecord, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let mut task = read::<CheckRecord>(&tx, "installation_checks", id)?.ok_or("安装任务已删除")?;
        let p = read(&tx, "projects", &task.check.project_id)?.ok_or("项目已删除")?;
        let mut d: DownloadRecord = read(&tx, "downloads", &task.check.download_id)?.ok_or("下载记录已删除")?;
        if !compatible(&task, &p, &d) || task.check.status != Waiting { return Err("安装任务状态已改变".into()); }
        if recovering {
            // A crash between starting ShellExecuteEx and committing its result leaves launch unknown.
            task.check.status = Unconfirmed;
            task.check.message = "上次启动安装向导时程序退出，无法确认是否启动。请检查后手动确认。".into();
        } else {
            task.launched = true;
            task.exit_pending = exit_pending;
        }
        write(&tx, "installation_checks", id, &task)?;
        if let Some(j) = journal {
            d.status = j.previous_download.status.clone();
            d.error = None; d.updated_at = now();
            write(&tx, "downloads", &d.id, &d)?;
            if j.snapshot_enabled {
                tx.execute("INSERT INTO backups(id,json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
                    params![p.id, serde_json::to_string(&j.backup).map_err(storage_error)?]).map_err(storage_error)?;
            }
            tx.execute("DELETE FROM install_journals WHERE id=?1", [&p.id]).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(d)
    }
    pub(in crate::core) fn recover_installation_checks(&self) -> Result<(), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        tx.execute("UPDATE installation_checks SET json=json_set(json,'$.exit_pending',json('false')) WHERE json_extract(json,'$.exit_pending')=1", []).map_err(storage_error)?;
        tx.execute("UPDATE installation_checks SET json=json_set(json,'$.check.status','unconfirmed','$.check.message',
            '上次启动安装向导时程序退出，无法确认是否启动。请检查后手动确认。')
            WHERE json_extract(json,'$.launched')=0 AND json_extract(json,'$.check.status')='waiting'", []).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
    pub(in crate::core) fn record_installer_exit(&self, id: &str, exit: ProcessExit, boot_id: Option<String>) -> Result<bool, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let Some(mut task) = read::<CheckRecord>(&tx, "installation_checks", id)? else { return Ok(false); };
        if !task.exit_pending || !task.check.status.can_check() || exit == ProcessExit::Running { return Ok(false); }
        task.exit_pending = false;
        (task.check.status, task.check.message) = match exit {
            ProcessExit::Code(1602 | 1223) => (Cancelled, "已取消安装。".into()),
            ProcessExit::Code(1641 | 3010) => {
                task.reboot_id = boot_id;
                (RebootRequired, "安装器报告需要重启 Windows。重启后再核实版本。".into())
            }
            ProcessExit::Code(0) | ProcessExit::Unavailable => (Detecting, "正在检查本机版本；如果安装向导仍在运行，请继续完成。".into()),
            ProcessExit::Code(code) => (Failed, format!("安装器退出并报告错误（{code}）。")),
            ProcessExit::Running => unreachable!(),
        };
        write(&tx, "installation_checks", id, &task)?;
        tx.commit().map_err(storage_error)?;
        Ok(true)
    }
    pub(in crate::core) fn publish_installation_probe(&self, previous: &CheckRecord, next: &CheckRecord, binding: Option<Vec<Identity>>)
        -> Result<(bool, Option<DownloadRecord>), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let id = &previous.check.id;
        if read::<CheckRecord>(&tx, "installation_checks", id)?.as_ref() != Some(previous) { return Ok((false, None)); }
        let Some(p) = read(&tx, "projects", &previous.check.project_id)? else { return Ok((false, None)); };
        let Some(d) = read(&tx, "downloads", &previous.check.download_id)? else { return Ok((false, None)); };
        if !compatible(previous, &p, &d) || !previous.launched || !previous.check.status.can_check() { return Ok((false, None)); }
        if previous.exit_pending && !(previous.check.status == Waiting && next.check.status == Unconfirmed && binding.is_none()) {
            return Ok((false, None));
        }
        let installed = if let Some(identities) = binding { Some(publish(&tx, next, p, d, Some(identities))?.1) }
            else { write(&tx, "installation_checks", id, next)?; None };
        tx.commit().map_err(storage_error)?;
        Ok((true, installed))
    }
    pub(in crate::core) fn confirm_installation(&self, previous: &CheckRecord) -> Result<(Project, DownloadRecord), String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let id = &previous.check.id;
        if read::<CheckRecord>(&tx, "installation_checks", id)?.as_ref() != Some(previous) || !previous.check.status.can_check()
            || (!previous.launched && previous.check.status != Unconfirmed) { return Err("此安装任务已结束或被替换，不能确认".into()); }
        let p = read(&tx, "projects", &previous.check.project_id)?.ok_or("项目已删除")?;
        let d = read(&tx, "downloads", &previous.check.download_id)?.ok_or("下载记录已删除")?;
        if !compatible(previous, &p, &d) { return Err("安装记录已更改，不能用旧任务确认".into()); }
        let mut next = previous.clone();
        next.check.status = Manual; next.exit_pending = false; next.check.checked_at = Some(now());
        next.check.detected_version = None; next.check.source = Some("manual".into()); next.check.message = "已手动记录本次安装版本。".into();
        let result = publish(&tx, &next, p, d, None)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
    pub(in crate::core) fn sync_install_binding(&self, previous: &InstallBinding, detected: Option<&str>, checked_at: String) -> Result<bool, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let id = &previous.project_id;
        if read::<InstallBinding>(&tx, "installation_bindings", id)?.as_ref() != Some(previous) { return Ok(false); }
        let pending: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM installation_checks WHERE project_id=?1 AND {ACTIVE})"), [id], |r| r.get(0)).map_err(storage_error)?;
        if pending { return Ok(false); }
        let Some(mut p) = read::<Project>(&tx, "projects", id)? else { return Ok(false); };
        if p.management_mode != "software" || p.installed_source.as_deref() != Some("installer") || ProjectGuard::from(&p) != previous.guard { return Ok(false); }
        let mut binding = previous.clone();
        binding.checked_at = checked_at;
        if let Some(version) = detected {
            p.installed_version = Some(version.into()); selection::update_status(&mut p);
            binding.guard = ProjectGuard::from(&p);
            write(&tx, "projects", id, &p)?;
        }
        write(&tx, "installation_bindings", id, &binding)?;
        tx.commit().map_err(storage_error)?;
        Ok(detected.is_some())
    }
}

fn publish(db: &Connection, task: &CheckRecord, mut p: Project, mut d: DownloadRecord, identities: Option<Vec<Identity>>) -> Result<(Project, DownloadRecord), String> {
    p.installed_version = Some(task.check.expected_version.clone());
    p.installed_source = Some(if identities.is_some() { "installer" } else { "manual" }.into());
    p.error = None; selection::update_status(&mut p);
    d.status = "installed".into(); d.error = None; d.updated_at = now();
    write(db, "projects", &p.id, &p)?;
    write(db, "downloads", &d.id, &d)?;
    if let Some(identities) = identities {
        let b = InstallBinding { project_id: p.id.clone(), guard: ProjectGuard::from(&p), identities, checked_at: now() };
        db.execute("INSERT INTO installation_bindings(id,json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
            params![p.id, serde_json::to_string(&b).map_err(storage_error)?]).map_err(storage_error)?;
    } else {
        db.execute("DELETE FROM installation_bindings WHERE id=?1", [&p.id]).map_err(storage_error)?;
    }
    write(db, "installation_checks", &task.check.id, task)?;
    Ok((p, d))
}
