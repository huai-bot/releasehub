import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Downloads from '../src/pages/Downloads.svelte';
import ProjectDetail from '../src/components/ProjectDetail.svelte';
import { createPreviewSnapshot } from '../src/lib/preview';
import { latestInstallation, installationLabel, canConfirmInstallation } from '../src/lib/installation-state';
import type { InstallationCheck, InstallationStatus } from '../src/lib/types';

let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | undefined;
const snapshot = createPreviewSnapshot();
const project = snapshot.projects[0];
const download = { ...snapshot.downloads[0], kind: 'installer' as const, status: 'completed' as const, tag: 'v6.5.0' };
const base: InstallationCheck = { id: 'attempt-1', projectId: project.id, downloadId: download.id, expectedVersion: 'v6.5.0', previousVersion: '6.4.0', status: 'waiting', startedAt: '2026-11-19T12:00:00Z', checkedAt: null, detectedVersion: null, source: null, message: '安装器已打开，等待实际版本验证。' };
async function settle() { for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); } }
function button(text: string) { return [...target.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === text); }
beforeEach(() => { target = document.createElement('div'); document.body.append(target); cleanup = undefined; });
afterEach(async () => { await cleanup?.(); target.remove(); });
async function showDownloads(checks: InstallationCheck[] = [base], busy = false) {
  const onVerify = vi.fn(), onConfirmInstallation = vi.fn(), oninstall = vi.fn();
  const instance = mount(Downloads, { target, props: { downloads: [download], installations: checks, query: '', busy, onVerify, onConfirmInstallation, oninstall, onretry: vi.fn(), oncancel: vi.fn(), onfolder: vi.fn(), onmark: vi.fn(), onproject: vi.fn(), oncopy: vi.fn(), onversions: vi.fn() } });
  cleanup = async () => { await unmount(instance); }; await settle(); return { onVerify, onConfirmInstallation, oninstall };
}

describe('installation verification presentation', () => {
  it.each(['waiting', 'detecting', 'unconfirmed', 'rebootRequired'] as InstallationStatus[])('offers one verification primary action for %s', async status => {
    const check = { ...base, status }; const callbacks = await showDownloads([check]);
    expect(target.querySelector('[role="status"]')?.textContent).toBe(installationLabel(check));
    expect(target.querySelectorAll('.download-actions .primary')).toHaveLength(1);
    button('立即检测')!.click(); await settle(); expect(callbacks.onVerify).toHaveBeenCalledExactlyOnceWith(project.id);
    expect(button('打开安装程序')).toBeUndefined();
    expect(target.querySelector('details')?.open).toBe(false);
    button('标记已安装 v6.5.0')!.click(); await settle(); expect(callbacks.onConfirmInstallation).toHaveBeenCalledExactlyOnceWith(base.id);
    expect(target.querySelector('dialog')).toBeNull(); expect(target.querySelector('input[type="checkbox"]')).toBeNull();
  });
  it('keeps manual override and reopening secondary, disabled while busy', async () => {
    const callbacks = await showDownloads([base], true);
    for (const label of ['立即检测', '标记已安装 v6.5.0', '重新打开安装器']) { expect(button(label)?.disabled).toBe(true); button(label)?.click(); }
    expect(callbacks.onVerify).not.toHaveBeenCalled(); expect(callbacks.onConfirmInstallation).not.toHaveBeenCalled(); expect(callbacks.oninstall).not.toHaveBeenCalled();
  });
  it('shows verified evidence in collapsed details without a redundant record action', async () => {
    await showDownloads([{ ...base, status: 'verified', detectedVersion: '6.5.0', source: 'registry', message: '检测到目标版本。' }]);
    expect(target.textContent).toContain('安装版本已同步'); expect(target.querySelector('details')?.textContent).toContain('Windows 安装记录');
    expect(target.textContent).toContain('6.5.0'); expect(button('标记已安装 v6.5.0')).toBeUndefined();
  });
  it.each(['cancelled', 'failed', 'rebootRequired'] as InstallationStatus[])('never labels %s as success', async status => {
    await showDownloads([{ ...base, status }]);
    expect(target.querySelector('[role="status"]')?.textContent).not.toContain('已同步');
    expect(target.querySelector('[role="status"]')?.textContent).not.toContain('已验证');
    if (status === 'failed') { expect(button('立即检测')).toBeUndefined(); expect(button('打开安装程序')).toBeDefined(); }
  });
  it('hides superseded attempts and supports old snapshots without checks', async () => {
    await showDownloads([{ ...base, status: 'superseded' }]);
    expect(target.querySelector('[role="status"]')).toBeNull(); expect(button('打开安装程序')).toBeDefined();
  });
  it('does not mutate the input order while selecting the latest task', () => {
    const checks = [base, { ...base, id: 'new', startedAt: '2026-11-19T13:00:00Z' }];
    expect(latestInstallation(checks, 'projectId', project.id)?.id).toBe('new'); expect(checks[0].id).toBe(base.id);
    expect(canConfirmInstallation({ ...base, status: 'verified' })).toBe(false);
  });
  it('keeps project verification compact and offers direct callbacks', async () => {
    const onVerify = vi.fn(), onConfirmInstallation = vi.fn();
    const instance = mount(ProjectDetail, { target, props: { project, installations: [{ ...base, status: 'unconfirmed' }], busy: false, onVerify, onConfirmInstallation, onclose: vi.fn(), oncheck: vi.fn(), ondownload: vi.fn(), onupdate: vi.fn(), onmark: vi.fn(), onremove: vi.fn(), onopen: vi.fn(), onversions: vi.fn(), ondiagnostic: vi.fn(), onrun: vi.fn(), onexecutable: vi.fn(), onrestore: vi.fn(), ondeletebackup: vi.fn(), onSettings: vi.fn() } });
    cleanup = async () => { await unmount(instance); }; await settle();
    expect(target.querySelector('.version-summary')?.textContent).toContain(project.installedVersion);
    expect(target.querySelector('.installation-box details')?.hasAttribute('open')).toBe(false);
    button('立即检测')!.click(); button('标记已安装 v6.5.0')!.click();
    expect(onVerify).toHaveBeenCalledExactlyOnceWith(project.id); expect(onConfirmInstallation).toHaveBeenCalledExactlyOnceWith(base.id);
    expect(button('下载已选文件')).toBeUndefined();
    expect(target.querySelector('button[aria-label^="检查 "]')).toBeNull();
  });
});
