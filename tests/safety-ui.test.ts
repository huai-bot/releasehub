import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import InstallConfirmation from '../src/components/InstallConfirmation.svelte';
import BackupPanel from '../src/components/BackupPanel.svelte';
import BackupManagement from '../src/components/BackupManagement.svelte';
import ManagedVersions from '../src/components/ManagedVersions.svelte';
import ProjectSettings from '../src/components/ProjectSettings.svelte';
import Library from '../src/pages/Library.svelte';
import Downloads from '../src/pages/Downloads.svelte';
import { createPreviewSnapshot } from '../src/lib/preview';
import type { BackupRecord, InstallPreview } from '../src/lib/types';

const mocks = vi.hoisted(() => ({ dispatch: vi.fn() }));
vi.mock('../src/lib/bridge', () => ({ dispatch: mocks.dispatch }));
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | undefined;
const download = createPreviewSnapshot().downloads[0];
const plan: InstallPreview = { downloadId: download.id, projectId: download.projectId, targetPath: 'D:\\Software\\Fixture', backupPath: 'D:\\Data\\backups\\fixture', previousVersion: '1.0', nextVersion: '2.0', backupEnabled: true, backupBytes: 1024, packageBytes: 2048, availableBytes: 10_000_000, kind: 'portable', archiveRoots: ['.', 'app'], suggestedRoot: null, requiresRootSelection: true, legacyManaged: false };
const backup: BackupRecord = { projectId: download.projectId, downloadId: download.id, version: '1.0', installedSource: 'manual', targetPath: plan.targetPath, path: plan.backupPath!, createdAt: '2026-11-18T00:00:00Z', bytes: 1024, kind: 'portable', phase: 'ready', error: null };
async function settle() { for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); } }
beforeEach(() => {
  vi.clearAllMocks(); target = document.createElement('div'); document.body.append(target); cleanup = undefined;
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});
afterEach(async () => { await cleanup?.(); target.remove(); });
function button(text: string) { return [...target.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === text)!; }

describe('installation inputs', () => {
  it('requires only the missing archive root and no confirmation checkbox', async () => {
    mocks.dispatch.mockResolvedValue(plan);
    const confirm = vi.fn(async () => {});
    const app = mount(InstallConfirmation, { target, props: { download, busy: false, onclose: vi.fn(), onconfirm: confirm } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(mocks.dispatch).toHaveBeenCalledExactlyOnceWith('preview_install', { id: download.id });
    const submit = target.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(submit.disabled).toBe(true);
    const select = target.querySelector('select')!; select.value = 'app'; select.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
    expect(submit.disabled).toBe(false);
    target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(confirm).toHaveBeenCalledExactlyOnceWith(plan, 'app');
    expect(mocks.dispatch.mock.calls.map(([action]) => action)).toEqual(['preview_install']);
  });
  it('editing the target invalidates the preview and can be checked again', async () => {
    mocks.dispatch.mockResolvedValue({ ...plan, suggestedRoot: '.', requiresRootSelection: false });
    const app = mount(InstallConfirmation, { target, props: { download, busy: false, onclose: vi.fn(), onconfirm: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    const field = target.querySelector<HTMLInputElement>('input:not([type="checkbox"])')!; field.value = 'D:\\Other'; field.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    expect(target.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
    mocks.dispatch.mockResolvedValue({ ...plan, targetPath: 'D:\\Other', suggestedRoot: '.', requiresRootSelection: false, backupEnabled: false, backupPath: null, backupBytes: 0 });
    button('检查目录').click(); await settle();
    expect(target.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false);
    expect(target.textContent).toContain('本次不会创建备份');
  });
  it('allows a validated legacy migration target and displays execution failures', async () => {
    const migration = { ...plan, legacyManaged: true, requiresRootSelection: false, suggestedRoot: '.' };
    const confirm = vi.fn(async () => {});
    const app = mount(InstallConfirmation, { target, props: { download, initialPlan: migration, actionError: '目标文件被占用，请关闭软件后重试', busy: false, onclose: vi.fn(), onconfirm: confirm } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(mocks.dispatch).not.toHaveBeenCalled();
    expect(target.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('目标文件被占用');
    target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(confirm).toHaveBeenCalledExactlyOnceWith(migration, '.');
  });
  it('browser preview makes no real filesystem dispatch', async () => {
    const app = mount(InstallConfirmation, { target, props: { download, preview: true, busy: false, onclose: vi.fn(), onconfirm: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(mocks.dispatch).not.toHaveBeenCalled(); expect(target.textContent).toContain('不会读取或修改本机软件');
  });
});
describe('backup actions', () => {
  it('opens an independent backup window from software cards', async () => {
    const project = createPreviewSnapshot().projects[0], open = vi.fn();
    const card = mount(Library, { target, props: { projects: [project], query: '', selectedId: null, busy: false, onadd: vi.fn(), onselect: vi.fn(), ondownload: vi.fn(), oncheck: vi.fn(), onbatchdownload: vi.fn(), onBackup: open } });
    await settle(); target.querySelector('.card-identity')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true })); await settle(); button('管理备份').click(); expect(open).toHaveBeenCalledWith(project); await unmount(card);
    mocks.dispatch.mockResolvedValue(backup);
    const update = vi.fn(async () => {}), restore = vi.fn(), remove = vi.fn();
    const app = mount(BackupManagement, { target, props: { project, revision: 0, busy: false, preview: false, error: '', onclose: vi.fn(), onupdate: update, onrestore: restore, ondelete: remove } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelector('[role="tablist"]')).toBeNull();
    const toggle = target.querySelector<HTMLInputElement>('[role="switch"]')!;
    expect(toggle.checked).toBe(false); toggle.checked = true; toggle.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    expect(update).toHaveBeenCalledExactlyOnceWith(project.id, { backupEnabled: true });
    button('恢复此备份').click(); button('删除备份').click();
    expect(restore).toHaveBeenCalledExactlyOnceWith(backup); expect(remove).toHaveBeenCalledExactlyOnceWith(backup);
    expect(target.querySelectorAll('dialog')).toHaveLength(1);
  });
  it('hides deleted downloads and counts only retained records', async () => {
    const downloads = createPreviewSnapshot().downloads;
    const app = mount(Downloads, { target, props: { downloads, query: '', busy: false, onretry: vi.fn(), oncancel: vi.fn(), oninstall: vi.fn(), onfolder: vi.fn(), onmark: vi.fn(), onproject: vi.fn(), oncopy: vi.fn(), onversions: vi.fn(), onDelete: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelectorAll('.download-item')).toHaveLength(downloads.filter(item => item.status !== 'deleted').length);
    expect(target.querySelector('.heading-count')?.textContent).toBe(String(downloads.filter(item => item.status !== 'deleted').length));
    expect(button('已删除')).toBeUndefined(); expect(target.textContent).not.toContain('下载完成，不代表已经安装');
  });
  it('recovery-required backups may be restored but not deleted', async () => {
    mocks.dispatch.mockResolvedValue({ ...backup, phase: 'recoveryRequired' });
    const restore = vi.fn(), remove = vi.fn();
    const app = mount(BackupPanel, { target, props: { projectId: backup.projectId, busy: false, onrestore: restore, ondelete: remove } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(button('删除备份').disabled).toBe(true); expect(button('恢复此备份').disabled).toBe(false);
    button('恢复此备份').click(); expect(restore).toHaveBeenCalledOnce(); expect(remove).not.toHaveBeenCalled();
    expect(mocks.dispatch.mock.calls.map(([action]) => action)).toEqual(['get_backup']);
  });
  it('read failures remain visible without destructive actions', async () => {
    mocks.dispatch.mockRejectedValue(new Error('备份目录不可用'));
    const app = mount(BackupPanel, { target, props: { projectId: backup.projectId, busy: false, onrestore: vi.fn(), ondelete: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('备份目录不可用'); expect(button('删除备份')).toBeUndefined();
  });
});

describe('compact settings and downloads', () => {
  it('keeps backup choice out of settings and retains metadata controls', async () => {
    const project = createPreviewSnapshot().projects[0];
    const app = mount(ProjectSettings, { target, props: { project, busy: false, error: '', onclose: vi.fn(), onupdate: vi.fn(async () => {}), onmark: vi.fn(), onexecutable: vi.fn(), onversions: vi.fn(), ondiagnostic: vi.fn(), onremove: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
    expect(target.querySelectorAll('select')).toHaveLength(3);
    expect(target.querySelector('details')?.open).toBe(false);
    expect(target.textContent).toContain('修改本机版本');
  });
  it('activates a prepared version on the first click', async () => {
    const project = { ...createPreviewSnapshot().projects[0], installedSource: 'managed' as const };
    const activate = vi.fn(async () => true);
    const app = mount(ManagedVersions, { target, props: { project, versions: [{ downloadId: 'prepared', version: '2.0', path: 'D:\\Apps\\2', active: false }], loading: false, error: '', actionError: '', busy: false, preview: false, onclose: vi.fn(), onrefresh: vi.fn(), onopen: vi.fn(), onactivate: activate } }); cleanup = async () => { await unmount(app); }; await settle();
    button('设为当前版本').click(); await settle();
    expect(activate).toHaveBeenCalledExactlyOnceWith('prepared');
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
  });
  it('retains external import, hashes, errors and secondary actions behind disclosure', async () => {
    const source = createPreviewSnapshot().downloads[0];
    const external = { ...source, id: 'external', status: 'external' as const, error: '发送回执不明确', sha256: 'abcd' };
    const importFile = vi.fn(), retry = vi.fn(), copy = vi.fn();
    const app = mount(Downloads, { target, props: { downloads: [external], query: '', busy: false, onretry: vi.fn(), oncancel: vi.fn(), oninstall: vi.fn(), onfolder: vi.fn(), onmark: vi.fn(), onproject: vi.fn(), oncopy: copy, onversions: vi.fn(), onDelete: vi.fn(), onexternalimport: importFile, onexternalretry: retry } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelector('details')?.open).toBe(false);
    expect(target.querySelectorAll('.download-actions button')).toHaveLength(1);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('发送回执不明确');
    button('导入已下载文件').click(); button('重新发送到下载器').click();
    target.querySelector<HTMLButtonElement>('[aria-label="复制 SHA-256"]')!.click();
    expect(importFile).toHaveBeenCalledWith(external); expect(retry).toHaveBeenCalledWith(external); expect(copy).toHaveBeenCalledWith('abcd');
  });
});
