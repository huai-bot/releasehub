import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, isTauri: mocks.isTauri }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
import { dispatch, watchChanges } from './bridge';

beforeEach(() => {
  mocks.invoke.mockReset(); mocks.isTauri.mockReset(); mocks.listen.mockReset();
  mocks.isTauri.mockReturnValue(true);
});

describe('single desktop command', () => {
  it('uses the dispatch action/payload envelope, not ad-hoc commands', async () => {
    mocks.invoke.mockResolvedValue(null);
    await dispatch('cancel_download', { id: 'download-1' });
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('dispatch', { action: 'cancel_download', payload: { id: 'download-1' } });
  });
  it('preserves the shared payload shapes for project, settings and build operations', async () => {
    mocks.invoke.mockResolvedValue(null);
    const input = { repository: 'owner/repo', channel: 'stable', preference: 'portable' } as const;
    await dispatch('add_project', { input });
    await dispatch('update_project', { id: 'project-1', changes: { selectedAssetId: 123 } });
    await dispatch('save_settings', { settings: { autoDownload: false } });
    await dispatch('check_updates', { projectIds: ['project-1'], allowAutoDownload: true });
    await dispatch('download_asset', { projectId: 'project-1', assetId: 123 });
    await dispatch('mark_installed', { projectId: 'project-1', version: 'v1.2.3', path: 'C:\\Apps\\example' });
    await dispatch('create_build_task', { projectId: 'project-1', sourceRef: 'main', instructions: 'Build Windows x64' });
    expect(mocks.invoke.mock.calls).toEqual([
      ['dispatch', { action: 'add_project', payload: { input } }],
      ['dispatch', { action: 'update_project', payload: { id: 'project-1', changes: { selectedAssetId: 123 } } }],
      ['dispatch', { action: 'save_settings', payload: { settings: { autoDownload: false } } }],
      ['dispatch', { action: 'check_updates', payload: { projectIds: ['project-1'], allowAutoDownload: true } }],
      ['dispatch', { action: 'download_asset', payload: { projectId: 'project-1', assetId: 123 } }],
      ['dispatch', { action: 'mark_installed', payload: { projectId: 'project-1', version: 'v1.2.3', path: 'C:\\Apps\\example' } }],
      ['dispatch', { action: 'create_build_task', payload: { projectId: 'project-1', sourceRef: 'main', instructions: 'Build Windows x64' } }],
    ]);
  });
  it('reads managed versions without inventing an active version or directory', async () => {
    const versions = [{ downloadId: 'download-1', version: 'v1.2.3', path: 'C:\\ReleaseHub\\data\\apps\\project-1\\v1.2.3', active: false }];
    mocks.invoke.mockResolvedValue(versions);
    expect(await dispatch('list_managed_versions', { projectId: 'project-1' })).toEqual(versions);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('dispatch', { action: 'list_managed_versions', payload: { projectId: 'project-1' } });
  });
  it('sends only project/download identity and explicit confirmation for managed actions', async () => {
    mocks.invoke.mockResolvedValue(null);
    await dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true });
    await dispatch('open_managed_version', { projectId: 'project-1', downloadId: 'download-1' });
    expect(mocks.invoke.mock.calls).toEqual([
      ['dispatch', { action: 'activate_managed_version', payload: { projectId: 'project-1', downloadId: 'download-1', confirmed: true } }],
      ['dispatch', { action: 'open_managed_version', payload: { projectId: 'project-1', downloadId: 'download-1' } }],
    ]);
  });
  it('never simulates managed-directory reads, switching, or opening without a desktop host', async () => {
    mocks.isTauri.mockReturnValue(false);
    await expect(dispatch('list_managed_versions', { projectId: 'project-1' })).rejects.toThrow('桌面服务未连接');
    await expect(dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true })).rejects.toThrow('桌面服务未连接');
    await expect(dispatch('open_managed_version', { projectId: 'project-1', downloadId: 'download-1' })).rejects.toThrow('桌面服务未连接');
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it('preserves a real managed-version rejection rather than returning success', async () => {
    mocks.invoke.mockRejectedValue(new Error('已准备的程序目录不存在'));
    await expect(dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true })).rejects.toThrow('已准备的程序目录不存在');
  });
  it('retains the explicit installer confirmation in the host request', async () => {
    mocks.invoke.mockResolvedValue({ installed: false, message: 'installer opened', kind: 'installer', path: 'test.exe' });
    const result = await dispatch('install_download', { id: 'download-1', confirmed: true });
    expect(mocks.invoke).toHaveBeenCalledWith('dispatch', { action: 'install_download', payload: { id: 'download-1', confirmed: true } });
    expect(result.installed).toBe(false);
  });
  it('rejects missing desktop services even in the test development environment', async () => {
    mocks.isTauri.mockReturnValue(false);
    await expect(dispatch('snapshot', {})).rejects.toThrow('桌面服务未连接');
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it('passes real host failures through rather than returning demo success', async () => {
    mocks.invoke.mockRejectedValue(new Error('网络请求失败'));
    await expect(dispatch('check_updates', { allowAutoDownload: true })).rejects.toThrow('网络请求失败');
  });
});

describe('live state subscription cleanup', () => {
  it('releases already-opened listeners if the next listener fails', async () => {
    const release = vi.fn();
    mocks.listen.mockResolvedValueOnce(release).mockRejectedValueOnce(new Error('event unavailable'));
    await expect(watchChanges(() => {})).rejects.toThrow('event unavailable');
    expect(release).toHaveBeenCalledOnce();
  });
  it('returns one cleanup for both supported event types', async () => {
    const releaseA = vi.fn(); const releaseB = vi.fn(); const changed = vi.fn();
    mocks.listen.mockResolvedValueOnce(releaseA).mockResolvedValueOnce(releaseB);
    const release = await watchChanges(changed);
    expect(mocks.listen.mock.calls.map((call) => call[0])).toEqual(['releasehub:changed', 'releasehub:download']);
    release();
    expect(releaseA).toHaveBeenCalledOnce(); expect(releaseB).toHaveBeenCalledOnce();
  });
});
