import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import InstallConfirmation from '../src/components/InstallConfirmation.svelte';
import BackupPanel from '../src/components/BackupPanel.svelte';
import ProjectDetail from '../src/components/ProjectDetail.svelte';
import Library from '../src/pages/Library.svelte';
import Downloads from '../src/pages/Downloads.svelte';
import { createPreviewSnapshot } from '../src/lib/preview';
import type { BackupRecord, InstallPreview } from '../src/lib/types';

const mocks = vi.hoisted(() => ({ dispatch: vi.fn() }));
vi.mock('../src/lib/bridge', () => ({ dispatch: mocks.dispatch }));
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | undefined;
const download = createPreviewSnapshot().downloads[0];
const plan: InstallPreview = { downloadId: download.id, projectId: download.projectId, targetPath: 'D:\\Software\\Fixture', backupPath: 'D:\\Data\\backups\\fixture', previousVersion: '1.0', nextVersion: '2.0', backupBytes: 1024, packageBytes: 2048, availableBytes: 10_000_000, kind: 'portable', archiveRoots: ['.', 'app'], suggestedRoot: null, requiresRootSelection: true, legacyManaged: false };
const backup: BackupRecord = { projectId: download.projectId, downloadId: download.id, version: '1.0', installedSource: 'manual', targetPath: plan.targetPath, path: plan.backupPath, createdAt: '2026-11-18T00:00:00Z', bytes: 1024, kind: 'portable', phase: 'ready', error: null };
async function settle() { for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); } }
beforeEach(() => {
  vi.clearAllMocks(); target = document.createElement('div'); document.body.append(target); cleanup = undefined;
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});
afterEach(async () => { await cleanup?.(); target.remove(); });
function button(text: string) { return [...target.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === text)!; }

describe('explicit install confirmation', () => {
  it('only previews until a root and overwrite confirmation are supplied', async () => {
    mocks.dispatch.mockResolvedValue(plan);
    const confirm = vi.fn(async () => {});
    const app = mount(InstallConfirmation, { target, props: { download, busy: false, onclose: vi.fn(), onconfirm: confirm } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(mocks.dispatch).toHaveBeenCalledExactlyOnceWith('preview_install', { id: download.id });
    const submit = target.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(submit.disabled).toBe(true);
    const select = target.querySelector('select')!; select.value = 'app'; select.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    expect(submit.disabled).toBe(true);
    const check = target.querySelector<HTMLInputElement>('input[type="checkbox"]')!; check.checked = true; check.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    expect(submit.disabled).toBe(false);
    target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(confirm).toHaveBeenCalledExactlyOnceWith(plan, 'app');
    expect(mocks.dispatch.mock.calls.map(([action]) => action)).toEqual(['preview_install']);
  });
  it('editing the target invalidates the confirmed preview', async () => {
    mocks.dispatch.mockResolvedValue({ ...plan, suggestedRoot: '.', requiresRootSelection: false });
    const app = mount(InstallConfirmation, { target, props: { download, busy: false, onclose: vi.fn(), onconfirm: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    const check = target.querySelector<HTMLInputElement>('input[type="checkbox"]')!; check.checked = true; check.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    const field = target.querySelector<HTMLInputElement>('input:not([type="checkbox"])')!; field.value = 'D:\\Other'; field.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    expect(check.checked).toBe(false); expect(target.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
  });
  it('browser preview makes no real filesystem dispatch', async () => {
    const app = mount(InstallConfirmation, { target, props: { download, preview: true, busy: false, onclose: vi.fn(), onconfirm: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(mocks.dispatch).not.toHaveBeenCalled(); expect(target.textContent).toContain('不会读取或修改本机软件');
  });
});
describe('backup actions', () => {
  it('offers backup management on software cards and opens the management tab directly', async () => {
    const project = createPreviewSnapshot().projects[0], open = vi.fn();
    const card = mount(Library, { target, props: { projects: [project], query: '', selectedId: null, busy: false, onadd: vi.fn(), onselect: vi.fn(), ondownload: vi.fn(), oncheck: vi.fn(), onbatchdownload: vi.fn(), onBackup: open } });
    await settle(); target.querySelector('.card-identity')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true })); await settle(); button('管理备份').click(); expect(open).toHaveBeenCalledWith(project); await unmount(card);
    mocks.dispatch.mockResolvedValue(backup);
    const app = mount(ProjectDetail, { target, props: { project, busy: false, initialTab: 'manage', onclose: vi.fn(), oncheck: vi.fn(), ondownload: vi.fn(), onupdate: vi.fn(), onmark: vi.fn(), onremove: vi.fn(), onopen: vi.fn(), onversions: vi.fn(), ondiagnostic: vi.fn(), onrun: vi.fn(), onexecutable: vi.fn(), onrestore: vi.fn(), ondeletebackup: vi.fn() } }); cleanup = async () => { await unmount(app); }; await settle();
    expect(target.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('管理');
    expect(button('恢复此备份')).toBeDefined(); expect(button('删除备份')).toBeDefined();
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
