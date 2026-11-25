import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import App from '../src/App.svelte';
import Sidebar from '../src/components/Sidebar.svelte';
import TitleBar from '../src/components/TitleBar.svelte';
import ProxySettings from '../src/components/ProxySettings.svelte';
import ExternalDownloadSettings from '../src/components/ExternalDownloadSettings.svelte';
import { createPreviewSnapshot } from '../src/lib/preview';
import { hasProjectUpdate } from '../src/lib/ui';
import { defaultNavigationOrder, moveNavigation, normalizeNavigation } from '../src/lib/navigation';
import type { InstallationCheck, ProxyTestResult, Settings } from '../src/lib/types';

const mocks = vi.hoisted(() => ({
  dispatch: vi.fn(), minimized: vi.fn(async () => {}), maximized: vi.fn(async () => {}), closed: vi.fn(async () => {}),
  theme: vi.fn(async () => {}), resizeCleanup: vi.fn(),
}));
vi.mock('../src/lib/bridge', () => ({ dispatch: mocks.dispatch, desktopAvailable: () => true, watchChanges: async () => () => {} }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  isMaximized: async () => false, onResized: async () => mocks.resizeCleanup, setTheme: mocks.theme,
  minimize: mocks.minimized, toggleMaximize: mocks.maximized, close: mocks.closed,
}) }));
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | undefined;
async function settle() { for (let count = 0; count < 12; count++) { await Promise.resolve(); flushSync(); } }
function button(label: string) {
  const matches = (item: HTMLButtonElement) => item.getAttribute('aria-label') === label || item.textContent?.trim() === label;
  const item = [...(target.querySelector('dialog[open]')?.querySelectorAll<HTMLButtonElement>('button') ?? [])].find(matches)
    ?? [...target.querySelectorAll<HTMLButtonElement>('button')].find(matches);
  if (!item) throw new Error(`Missing button ${label}`);
  return item;
}
async function input(selector: string, value: string) {
  const item = target.querySelector<HTMLInputElement | HTMLSelectElement>(selector)!;
  item.value = value; item.dispatchEvent(new Event(item.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true })); await settle();
}
function submit() { target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); }
const success: ProxyTestResult = { ok: true, mode: 'manual', source: 'manual', target: 'https://api.github.com/rate_limit', status: 200, latencyMs: 17, message: 'GitHub 连接正常' };

beforeEach(() => {
  vi.clearAllMocks(); cleanup = undefined;
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  target = document.createElement('div'); document.body.append(target);
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) });
});
afterEach(async () => { await cleanup?.(); target.remove(); });

describe('startup and desktop navigation', () => {
  it('marks all update versions read across filters and can mute or resume a single subscription', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'updates';
    const software = snapshot.projects[0];
    const files = snapshot.projects.find(project => project.managementMode === 'files')!;
    files.fileStatus = 'updateAvailable';
    const versions = snapshot.projects.filter(hasProjectUpdate).map(project => ({ projectId: project.id, tag: project.release!.tag }));
    mocks.dispatch.mockImplementation(async (action: string, payload) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'mark_updates_read') { snapshot.readUpdateTags = Object.fromEntries(payload.versions.map((row: {projectId:string;tag:string}) => [row.projectId,row.tag])); return payload.versions.length; }
      if (action === 'update_project') { Object.assign(software, payload.changes); return software; }
      if (action === 'check_updates') return { checked: 1, updates: 1, unreadUpdates: 0, errors: 0 };
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const nav = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('nav button')].find(item => item.querySelector('span')?.textContent === label)!;
    const badge = () => nav('可更新').querySelector('.nav-count')?.textContent;
    const active = () => target.querySelector<HTMLElement>('#main-content:not([hidden])')!;
    expect(badge()).toBe(String(versions.length));
    active().querySelectorAll<HTMLButtonElement>('[aria-label="资源类型筛选"] button')[2].click(); await settle();
    expect(active().querySelectorAll('.update-item')).toHaveLength(1);
    button('全部已读').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('mark_updates_read', { versions });
    expect(badge()).toBeUndefined(); expect(button('全部已读').disabled).toBe(true);
    expect(active().querySelectorAll('.update-item.read')).toHaveLength(1);
    nav('资源库').click(); await settle();
    const menu = async () => { active().querySelector<HTMLButtonElement>('.card-more')!.click(); await settle(); };
    await menu(); button('屏蔽更新').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('update_project',{ id: software.id, changes: { updatesMuted: true } });
    nav('可更新').click(); await settle(); active().querySelector<HTMLButtonElement>('[aria-label="资源类型筛选"] button')!.click(); await settle();
    expect(active().querySelectorAll('.update-item')).toHaveLength(versions.length - 1);
    nav('资源库').click(); await settle(); await menu(); button('恢复更新').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('check_updates',{ projectIds: [software.id], allowAutoDownload: false });
    expect(badge()).toBeUndefined();
    nav('可更新').click(); await settle(); expect(active().querySelectorAll('.update-item')).toHaveLength(versions.length);
  });
  it('opens the saved page only on startup and applies changed preferences on the next start', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'agent';
    mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_settings') { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...payload.settings } }; return snapshot.settings; }
      throw new Error(`Unexpected action ${action}`);
    });
    let instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    expect(target.querySelector('h1')?.textContent).toBe('AI 连接');
    button('设置').click(); await settle();
    await input('#startup-page', 'library');
    expect(mocks.dispatch).toHaveBeenCalledWith('save_settings', { settings: { startupPage: 'library' } });
    expect(target.querySelector('h1')?.textContent).toBe('设置');
    button('刷新本机状态').click(); await settle();
    expect(target.querySelector('h1')?.textContent).toBe('设置');
    await cleanup();
    instance = mount(App, { target }); await settle();
    expect(target.querySelector('h1')?.textContent).toContain('资源库');
  });

  it('keeps resource filters and search when visiting another page', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    mocks.dispatch.mockImplementation(async (action: string) => { if (action === 'snapshot') return snapshot; throw new Error(`Unexpected action ${action}`); });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const nav = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('nav button')].find(item => item.querySelector('span')?.textContent === label)!;
    const active = () => target.querySelector<HTMLElement>('#main-content:not([hidden])')!;
    expect(active().querySelectorAll('.project-card')).toHaveLength(snapshot.projects.filter(project => project.managementMode !== 'files').length);
    [...active().querySelectorAll<HTMLButtonElement>('[aria-label="资源类型筛选"] button')].find(item => item.textContent?.trim().startsWith('文件'))!.click(); await settle();
    expect(active().querySelectorAll('.project-card')).toHaveLength(1);
    expect(active().querySelector('.project-card')?.getAttribute('aria-label')).toBe('文件资料（界面示例）');
    const search = target.querySelector<HTMLInputElement>('.titlebar input')!;
    search.value = 'manual'; search.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    nav('下载').click(); await settle(); nav('资源库').click(); await settle();
    expect(active().querySelector('[aria-label="资源类型筛选"] [aria-pressed="true"]')?.textContent?.trim()).toMatch(/^文件/);
    expect(search.value).toBe('manual');
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'snapshot')).toHaveLength(1);
  });

  it('uses one soft-UI material with the cobalt accent and clears only page cache', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'home'; snapshot.settings.colorScheme = 'teal'; snapshot.settings.material = 'mica';
    mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_settings') { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...payload.settings } }; return snapshot.settings; }
      if (action === 'github_cache_stats') return { bytes: 4096, entries: 2 };
      if (action === 'github_cache_clear') return null;
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    expect(document.documentElement.dataset.colorScheme).toBe('blue');
    expect(document.documentElement.dataset.material).toBe('soft');
    expect(document.documentElement.dataset.blur).toBe('false');
    button('设置').click(); await settle();
    expect(target.querySelector('.palette-option, .material-option')).toBeNull();
    button('清理页面缓存').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('github_cache_clear', {});
    expect(target.textContent).toContain('下载文件和订阅保持不变');
  });

  it('keeps clicks separate from launching and exposes accessible context actions', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    snapshot.projects[0].executablePath = 'D:\\Fixture\\app.exe';
    mocks.dispatch.mockImplementation(async (action: string) => { if (action === 'snapshot') return snapshot; if (action === 'open_project') return null; throw new Error(`Unexpected action ${action}`); });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const identity = target.querySelector<HTMLButtonElement>('.card-identity')!;
    expect(target.querySelector('[aria-label="资源类型筛选"]')?.textContent).not.toContain('全部');
    expect(target.querySelector('.project-card .status-badge')).toBeNull();
    identity.click(); await settle(); expect(target.querySelector('.detail-panel')).toBeNull();
    identity.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('open_project', { projectId: snapshot.projects[0].id });
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'open_project')).toHaveLength(1);
    identity.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true })); await settle();
    expect(target.querySelector('[role="menu"]')).not.toBeNull();
    expect(document.activeElement?.textContent).toContain('打开软件');
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain('查看详情');
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); await settle();
    expect(target.querySelector('[role="menu"]')).toBeNull(); expect(document.activeElement).toBe(identity);
    identity.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 100, clientY: 100 })); await settle();
    const details = [...target.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(item => item.textContent?.includes('查看详情'))!;
    details.click(); await settle(); expect(target.querySelector('.detail-panel')).not.toBeNull();
  });

  it('adjusts the automatic check interval from the home knob and switch', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'home'; snapshot.settings.checkIntervalMinutes = 60;
    mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_settings') { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...payload.settings } }; return snapshot.settings; }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const knob = target.querySelector<HTMLElement>('[role="slider"][aria-label="自动检查间隔"]')!;
    expect(knob.getAttribute('aria-valuenow')).toBe('60');
    knob.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); await settle();
    expect(knob.getAttribute('aria-valuenow')).toBe('120');
    expect(target.querySelector('.target-line')?.textContent).toContain('2 小时');
    await new Promise((resolve) => setTimeout(resolve, 500)); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('save_settings', { settings: { checkIntervalMinutes: 120 } });
    target.querySelector<HTMLButtonElement>('[role="switch"][aria-label="自动检查"]')!.click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('save_settings', { settings: { checkIntervalMinutes: 0 } });
    expect(target.querySelector('.cadence')?.classList.contains('off')).toBe(true);
    target.querySelector<HTMLButtonElement>('[role="switch"][aria-label="自动检查"]')!.click(); await settle();
    const saves = mocks.dispatch.mock.calls.filter((call: unknown[]) => call[0] === 'save_settings').map((call: unknown[]) => call[1]);
    expect(saves).toEqual([{ settings: { checkIntervalMinutes: 120 } }, { settings: { checkIntervalMinutes: 0 } }, { settings: { checkIntervalMinutes: 120 } }]);
  });

  it('keeps every workspace page once and persists keyboard and drag ordering', async () => {
    expect(normalizeNavigation(['github', 'github', 'unknown'])).toEqual(['github', 'home', 'library', 'updates', 'downloads', 'agent']);
    expect(moveNavigation(defaultNavigationOrder, 'github', 'home')[0]).toBe('github');
    const onreorder = vi.fn(async () => true);
    const instance = mount(Sidebar, { target, props: { snapshot: createPreviewSnapshot(), page: 'home', preview: false, onnavigate: vi.fn(), onadd: vi.fn(), ondata: vi.fn(), onreorder } });
    cleanup = async () => { await unmount(instance); }; await settle();
    const nav = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('nav button')].find(item => item.querySelector('span')?.textContent === label)!;
    nav('GitHub').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, bubbles: true })); await settle();
    expect(onreorder).toHaveBeenLastCalledWith(['home', 'library', 'updates', 'github', 'downloads', 'agent']);
    const start = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(start, 'dataTransfer', { value: { effectAllowed: '', setData: vi.fn() } });
    nav('GitHub').dispatchEvent(start); await settle();
    nav('首页').dispatchEvent(new Event('drop', { bubbles: true, cancelable: true })); await settle();
    expect(onreorder).toHaveBeenLastCalledWith(['github', 'home', 'library', 'updates', 'downloads', 'agent']);
    expect(target.querySelector('[aria-live]')?.textContent).toContain('已保存');
  });

  it('persists per-file connections and saves external downloader configuration', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'home';
    mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_settings') { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...payload.settings } }; return snapshot.settings; }
      if (action === 'test_external_downloader') return { ok: true, message: '端口可连接，接口尚未验证' };
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('设置').click(); await settle(); await input('#download-connections', '3');
    expect(mocks.dispatch).toHaveBeenCalledWith('save_settings', { settings: { downloadConnections: 3 } });
    await input('#downloader-name', '本机测试下载器'); button('保存下载器配置').click(); await settle();
    expect(snapshot.settings.externalDownloader?.name).toBe('本机测试下载器');
    button('测试下载器连接').click(); await settle(); expect(mocks.dispatch).toHaveBeenCalledWith('test_external_downloader', { config: snapshot.settings.externalDownloader });
    button('手动下载默认使用外部下载器').click(); await settle(); expect(snapshot.settings.useExternalDownloader).toBe(true);
  });

  it('sends an external download link once from the deliberate download click', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library'; snapshot.settings.useExternalDownloader = true;
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'send_external_download') return { ...snapshot.downloads[0], status: 'external' };
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const project = snapshot.projects.find(item => item.release?.assets.some(asset => asset.id === item.selectedAssetId && (asset.downloadable ?? asset.compatible)))!;
    const card = [...target.querySelectorAll<HTMLElement>('.project-card')].find(item => item.textContent?.includes(project.name))!;
    card.querySelector<HTMLButtonElement>('.card-identity')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await settle();
    button('下载已选文件').click(); await settle();
    expect(target.querySelector('dialog[open]')).toBeNull();
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'send_external_download')).toHaveLength(1);
    expect(mocks.dispatch).toHaveBeenCalledWith('send_external_download', { projectId: project.id, assetId: project.selectedAssetId, confirmed: true });
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'download_asset')).toHaveLength(0);
  });

  it('offers import and one-click resend for external tasks without claiming completion', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'downloads';
    snapshot.downloads = [{ ...snapshot.downloads[0], status: 'external', path: null, sha256: null, verified: false }];
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'import_external_download') return null;
      if (action === 'retry_external_download') return snapshot.downloads[0];
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    expect(target.textContent).toContain('已交给外部下载器'); expect(target.querySelector('.download-progress')).toBeNull();
    button('导入已下载文件').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('import_external_download', { id: snapshot.downloads[0].id, confirmed: true });
    button('重新发送到下载器').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('retry_external_download', { id: snapshot.downloads[0].id, confirmed: true });
    expect(target.querySelector('dialog[open]')).toBeNull();
  });

  it('persists all library layouts and restores the last choice on restart', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_settings') { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...payload.settings } }; return snapshot.settings; }
      throw new Error(`Unexpected action ${action}`);
    });
    let instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    for (const [label, layout] of [['双列', 'two'], ['三列', 'three'], ['列表', 'list']] as const) {
      button(label).click(); await settle();
      expect(mocks.dispatch).toHaveBeenCalledWith('save_settings', { settings: { libraryLayout: layout } });
      expect(target.querySelector('#main-content .library-card-grid')?.classList.contains(`layout-${layout}`)).toBe(true);
      expect(button(label).getAttribute('aria-pressed')).toBe('true');
    }
    await cleanup(); instance = mount(App, { target }); await settle();
    expect(button('列表').getAttribute('aria-pressed')).toBe('true');
  });

  it('shows description-only notes without the original title and restores it after clearing', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    const project = snapshot.projects[0];
    mocks.dispatch.mockImplementation(async (action: string, payload: { repository?: string; name?: string; description?: string }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_repository_note') {
        const note = { repository: payload.repository!.toLowerCase(), name: payload.name!, description: payload.description!, updatedAt: new Date().toISOString() };
        snapshot = { ...snapshot, repositoryNotes: note.name || note.description ? [note] : [] };
        return note.name || note.description ? note : null;
      }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const card = () => target.querySelector<HTMLElement>('#main-content .project-card')!;
    const edit = async () => { card().querySelector<HTMLButtonElement>('.card-more')!.click(); await settle(); button('编辑备注').click(); await settle(); };
    await edit(); await input('#note-description', '  我的转码工具  ');
    target.querySelector('dialog form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('save_repository_note', { repository: `${project.owner}/${project.repo}`, name: '', description: '我的转码工具' });
    expect(target.querySelector('dialog')).toBeNull();
    expect(card().querySelector('strong')).toBeNull(); expect(card().querySelector('small')?.textContent).toBe('我的转码工具');
    expect(card().getAttribute('aria-label')).toBe('我的转码工具'); expect(card().textContent).not.toContain(project.name);
    const search = target.querySelector<HTMLInputElement>('.titlebar input')!;
    search.value = '我的转码'; search.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    expect(target.querySelectorAll('#main-content .project-card')).toHaveLength(1);
    search.value = ''; search.dispatchEvent(new Event('input', { bubbles: true })); await settle();
    card().querySelector('.card-identity')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); await settle();
    expect(target.querySelector('.detail-identity h2')).toBeNull(); expect(target.querySelector('.detail-identity p')?.textContent).toBe('我的转码工具');
    button('关闭资源详情').click(); await settle();
    await edit(); await input('#note-name', '本地名称'); await input('#note-description', '简短用途');
    target.querySelector('dialog form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(card().querySelector('strong')?.textContent).toBe('本地名称'); expect(card().querySelector('small')?.textContent).toBe('简短用途');
    expect(card().textContent).not.toContain(project.name);
    await edit(); await input('#note-name', ''); await input('#note-description', '');
    target.querySelector('dialog form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(card().querySelector('strong')?.textContent).toBe(project.name); expect(card().querySelector('small')).toBeNull();
  });

  it('shows only a description note, saves edits, searches notes and restores the original after clearing', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    const project = snapshot.projects[0]; const repository = `${project.owner}/${project.repo}`;
    snapshot.repositoryNotes = [{ repository: repository.toLowerCase(), name: '', description: '本地视频工具', updatedAt: new Date().toISOString() }];
    mocks.dispatch.mockImplementation(async (action: string, payload: { repository?: string; name?: string; description?: string }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'save_repository_note') {
        const note = payload.name || payload.description ? { repository: repository.toLowerCase(), name: payload.name!, description: payload.description!, updatedAt: new Date().toISOString() } : null;
        snapshot = { ...snapshot, repositoryNotes: note ? [note] : [] }; return note;
      }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const card = () => target.querySelector<HTMLElement>('#main-content .project-card')!;
    expect(card().querySelector('strong')).toBeNull(); expect(card().querySelector('small')?.textContent).toBe('本地视频工具');
    expect(card().getAttribute('aria-label')).toBe('本地视频工具'); expect(card().innerHTML).not.toContain(project.name);
    await input('.titlebar input', '本地视频工具'); expect(target.querySelectorAll('#main-content .project-card')).toHaveLength(1);
    const editNote = async () => {
      card().querySelector<HTMLButtonElement>('.card-identity')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true })); await settle();
      button('编辑备注').click(); await settle();
    };
    await editNote(); await input('#note-name', '  视频转换  '); await input('#note-description', '  常用工具  ');
    button('保存备注').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('save_repository_note', { repository, name: '视频转换', description: '常用工具' });
    await input('.titlebar input', '视频转换'); expect(card().querySelector('strong')?.textContent).toBe('视频转换');
    expect(card().querySelector('small')?.textContent).toBe('常用工具'); expect(card().textContent).not.toContain(project.name);
    await editNote(); await input('#note-name', ''); await input('#note-description', ''); button('保存备注').click(); await settle();
    expect(snapshot.repositoryNotes).toHaveLength(0); await input('.titlebar input', '');
    expect(card().querySelector('strong')?.textContent).toBe(project.name); expect(card().querySelector('small')).toBeNull();
  });

  it('keeps paths collapsed, discards a cancelled picker draft and saves without a checkbox', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    const project = snapshot.projects[0]; project.installedPath = null; project.executablePath = null; project.installedSource = null;
    const selection = { installedPath: 'D:\\Fixture\\App', executablePath: 'D:\\Fixture\\App\\app.exe' };
    mocks.dispatch.mockImplementation(async (action: string, payload: { version?: string; path?: string; executablePath?: string }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'pick_executable') return selection;
      if (action === 'get_backup') return null;
      if (action === 'mark_installed') {
        const saved = { ...project, installedVersion: payload.version!, installedPath: payload.path!, executablePath: payload.executablePath! };
        snapshot = { ...snapshot, projects: snapshot.projects.map(item => item.id === project.id ? saved : item) }; return saved;
      }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    target.querySelector<HTMLButtonElement>('#main-content .card-identity')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await settle();
    button('修改本机版本').click(); await settle();
    expect(target.querySelector<HTMLDetailsElement>('dialog details')?.open).toBe(false);
    target.querySelector('dialog details')!.setAttribute('open', '');
    button('选择 EXE').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('pick_executable', { projectId: project.id });
    expect(target.querySelector<HTMLInputElement>('#mark-path')!.value).toBe(selection.installedPath);
    expect(target.querySelector<HTMLInputElement>('#mark-executable')!.value).toBe(selection.executablePath);
    button('取消').click(); await settle();
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'mark_installed' || action === 'open_project')).toHaveLength(0);
    expect(snapshot.projects[0].installedPath).toBeNull();
    button('修改本机版本').click(); await settle(); expect(target.querySelector<HTMLInputElement>('#mark-path')!.value).toBe('');
    target.querySelector('dialog details')!.setAttribute('open', '');
    button('选择 EXE').click(); await settle(); await input('#mark-version', 'v2.0.0');
    expect(target.querySelector('dialog input[type="checkbox"]')).toBeNull();
    expect(button('保存版本').disabled).toBe(false);
    button('保存版本').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('mark_installed', { projectId: project.id, version: 'v2.0.0', path: selection.installedPath, executablePath: selection.executablePath });
    expect(target.querySelector('dialog[open]')).toBeNull(); expect(snapshot.projects[0].executablePath).toBe(selection.executablePath);
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'open_project')).toHaveLength(0);
  });

  it('saves version alone without resending hidden paths', async () => {
    let snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    const project = snapshot.projects[0]; project.installedPath = 'D:\\Fixture'; project.executablePath = 'D:\\Fixture\\app.exe';
    mocks.dispatch.mockImplementation(async (action: string, payload: { version?: string }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'mark_installed') { const saved = { ...project, installedVersion: payload.version! }; snapshot = { ...snapshot, projects: snapshot.projects.map(item => item.id === saved.id ? saved : item) }; return saved; }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    target.querySelector<HTMLButtonElement>('#main-content .card-identity')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await settle();
    button('修改本机版本').click(); await settle(); await input('#mark-version', 'v9'); button('保存版本').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('mark_installed', { projectId: project.id, version: 'v9' });
  });

  it('installs directly when preview already knows the target and archive root', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'downloads';
    const download = { ...snapshot.downloads[0], status: 'completed' as const, kind: 'portable' as const, installable: true }; snapshot.downloads = [download];
    const plan = { downloadId: download.id, projectId: download.projectId, targetPath: 'D:\\Fixture', previousVersion: '1', nextVersion: '2', backupEnabled: false, backupPath: null, backupBytes: 0, packageBytes: 1, availableBytes: 10000, kind: 'portable', archiveRoots: ['.'], suggestedRoot: '.', requiresRootSelection: false, legacyManaged: false };
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'preview_install') return plan;
      if (action === 'install_download') return { installed: true, message: '已安装' };
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('安装 / 更新').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('install_download', { id: download.id, confirmed: true, targetPath: plan.targetPath, previousVersion: '1', archiveRoot: '.' });
    expect(target.querySelector('dialog[open]')).toBeNull();
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'preview_install')).toHaveLength(1);
  });

  it('does not claim a saved executable when the native picker is cancelled', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'library';
    snapshot.projects[0].installedPath = null; snapshot.projects[0].executablePath = null; snapshot.projects[0].installedSource = null;
    mocks.dispatch.mockImplementation(async (action: string) => { if (action === 'snapshot') return snapshot; if (action === 'pick_project_executable') return null; throw new Error(`Unexpected action ${action}`); });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    target.querySelector<HTMLButtonElement>('#main-content .card-identity')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('pick_project_executable', { projectId: snapshot.projects[0].id });
    expect(target.querySelector('.toast-stack')?.textContent).not.toContain('已保存启动程序');
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'open_project')).toHaveLength(0);
  });

  it('uses native window controls and releases its resize listener', async () => {
    const instance = mount(TitleBar, { target, props: { title: 'GitHub' } });
    cleanup = async () => { await unmount(instance); }; await settle();
    button('最小化窗口').click(); button('最大化窗口').click(); button('关闭窗口').click(); await settle();
    expect(mocks.minimized).toHaveBeenCalledOnce(); expect(mocks.maximized).toHaveBeenCalledOnce(); expect(mocks.closed).toHaveBeenCalledOnce();
    expect(target.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true);
    await cleanup(); cleanup = undefined; expect(mocks.resizeCleanup).toHaveBeenCalledOnce();
  });
});

describe('external downloader preference regression', () => {
  const preset = { endpoint: 'http://127.0.0.1:15151/add', requestTemplate: '{"items":[{"type":"http","link":"{url}","downloadPage":"{pageUrl}","suggestedName":"{fileName}"}],"options":{"silentAdd":true,"silentStart":true}}', name: 'AB Download Manager' };
  async function show(enabled = false, save = vi.fn(async (_changes: Partial<Settings>) => true)) {
    const settings = { ...createPreviewSnapshot().settings, useExternalDownloader: enabled, externalDownloader: preset };
    const instance = mount(ExternalDownloadSettings, { target, props: { settings, busy: false, preview: false, onsave: save } });
    cleanup = async () => { await unmount(instance); }; await settle(); return { save, settings };
  }
  it('ignores backend property order and keeps the switch available', async () => {
    const { save } = await show();
    expect(button('保存下载器配置').disabled).toBe(true);
    expect(button('手动下载默认使用外部下载器').disabled).toBe(false);
    button('手动下载默认使用外部下载器').click(); await settle();
    expect(save).toHaveBeenCalledWith({ useExternalDownloader: true });
  });
  it('saves an edited configuration together with enabling, but does not save it while disabling', async () => {
    const { save } = await show(); await input('#downloader-name', '我的 AB');
    button('手动下载默认使用外部下载器').click(); await settle();
    expect(save).toHaveBeenCalledWith({ useExternalDownloader: true, externalDownloader: { ...preset, name: '我的 AB' } });
    await cleanup!(); cleanup = undefined;
    const second = await show(true); await input('#downloader-endpoint', 'invalid draft');
    button('手动下载默认使用外部下载器').click(); await settle();
    expect(second.save).toHaveBeenCalledWith({ useExternalDownloader: false });
  });
  it('keeps a failed draft and the persisted switch state, then allows a corrected retry', async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error('接口地址无效')).mockResolvedValue(true);
    await show(false, save); await input('#downloader-endpoint', 'wrong');
    button('手动下载默认使用外部下载器').click(); await settle();
    expect(button('手动下载默认使用外部下载器').getAttribute('aria-checked')).toBe('false');
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('接口地址无效');
    expect(target.querySelector<HTMLInputElement>('#downloader-endpoint')!.value).toBe('wrong');
    await input('#downloader-endpoint', preset.endpoint); button('手动下载默认使用外部下载器').click(); await settle();
    expect(save).toHaveBeenCalledTimes(2); expect(target.querySelector('[role="alert"]')).toBeNull();
  });
  it('does not send twice while a save is pending', async () => {
    let resolve!: (saved: boolean) => void;
    const save = vi.fn((_changes: Partial<Settings>) => new Promise<boolean>(done => { resolve = done; }));
    await show(false, save); button('手动下载默认使用外部下载器').click(); await settle();
    expect(button('手动下载默认使用外部下载器').disabled).toBe(true);
    button('手动下载默认使用外部下载器').click(); expect(save).toHaveBeenCalledOnce();
    resolve(false); await settle(); expect(button('手动下载默认使用外部下载器').disabled).toBe(false);
  });
});

describe('network proxy preferences', () => {
  async function show(settings = { mode: 'system', url: '', username: '' } as Settings['proxy'], saved = false, save = vi.fn(async () => true), result = success) {
    const ontest = vi.fn(async (_proxy: Settings['proxy'], _password?: string) => result);
    const instance = mount(ProxySettings, { target, props: { settings, passwordConfigured: saved, busy: false, preview: false, onsave: save, ontest } });
    cleanup = async () => { await unmount(instance); }; await settle(); return { save, ontest };
  }
  it('tests unsaved values separately, then saves and clears the password field', async () => {
    const { save, ontest } = await show();
    await input('#proxy-mode', 'manual'); await input('#proxy-url', ' http://127.0.0.1:7890 ');
    await input('input[autocomplete="off"]', 'demo'); await input('input[type="password"]', 'test-value');
    button('测试连接').click(); await settle();
    expect(ontest).toHaveBeenCalledWith({ mode: 'manual', url: 'http://127.0.0.1:7890', username: 'demo' }, 'test-value');
    expect(save).not.toHaveBeenCalled(); expect(target.querySelector('[role="status"]')?.textContent).toContain('17 ms');
    submit(); await settle();
    expect(save).toHaveBeenCalledWith({ mode: 'manual', url: 'http://127.0.0.1:7890', username: 'demo' }, 'test-value');
    expect(target.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe('');
  });
  it('keeps saved passwords omitted and only sends an empty value for explicit clearing', async () => {
    const { save, ontest } = await show({ mode: 'manual', url: 'socks5://127.0.0.1:1080', username: 'demo' }, true);
    button('测试连接').click(); await settle();
    expect(ontest.mock.calls[0][1]).toBeUndefined();
    target.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); await settle(); submit(); await settle();
    expect(save).toHaveBeenCalledWith({ mode: 'manual', url: 'socks5://127.0.0.1:1080', username: 'demo' }, '');
  });
  it('shows exhausted GitHub quota separately from a successful proxy connection', async () => {
    await show(undefined, false, undefined, { ...success, warning: '当前出口的 GitHub 未登录查询额度已用完。', retryAt: '2026-11-15T12:00:00Z' });
    button('测试连接').click(); await settle();
    expect(target.querySelector('.success-text')?.textContent).toContain('GitHub 连接正常');
    expect(target.querySelector('.danger-text')).toBeNull();
    expect(target.querySelector('.proxy-warning')?.textContent).toContain('未登录查询额度');
    expect(target.querySelector('.proxy-warning')?.textContent).toContain('预计');
    expect(target.querySelector('.proxy-warning')?.textContent).not.toContain('Invalid Date');
  });
  it('still displays a real proxy authentication failure as an error', async () => {
    await show(undefined, false, undefined, { ...success, ok: false, status: 407, message: '代理验证未通过，请检查账号和密码', warning: null });
    button('测试连接').click(); await settle();
    expect(target.querySelector('.danger-text')?.textContent).toContain('账号和密码');
    expect(target.querySelector('.proxy-warning')).toBeNull();
  });
  it('retains a failed draft and allows a corrected retry', async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error('代理地址格式不正确')).mockResolvedValue(true);
    await show(undefined, false, save);
    await input('#proxy-mode', 'manual'); await input('#proxy-url', 'wrong'); await input('input[type="password"]', 'draft-value');
    submit(); await settle();
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('代理地址格式不正确');
    expect(target.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe('draft-value');
    await input('#proxy-url', 'http://127.0.0.1:7890'); submit(); await settle();
    expect(save).toHaveBeenCalledTimes(2); expect(target.querySelector('[role="alert"]')).toBeNull();
  });
});

describe('desktop installation verification integration', () => {
  function fixture(status?: InstallationCheck['status']) {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'downloads';
    snapshot.downloads = [{ ...snapshot.downloads[0], kind: 'installer', status: 'completed', tag: 'v6.5.0' }];
    const record = snapshot.downloads[0];
    const check: InstallationCheck = { id: 'fixture-attempt', projectId: record.projectId, downloadId: record.id, expectedVersion: record.tag, previousVersion: '6.4.0', status: status ?? 'waiting', startedAt: '2026-11-19T12:00:00Z', checkedAt: null, detectedVersion: null, source: null, message: '等待实际版本验证' };
    snapshot.installations = status ? [check] : [];
    return { snapshot, check, record };
  }
  it('opens an EXE/MSI immediately without portable-directory preview or confirmation', async () => {
    const { snapshot, record } = fixture();
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'install_download') return { installed: false, installationId: 'fixture-attempt', message: '安装器已打开' };
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('打开安装程序').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('install_download', { id: record.id, targetPath: '', confirmed: true });
    expect(mocks.dispatch.mock.calls.some(([action]) => action === 'preview_install')).toBe(false);
    expect(target.querySelector('dialog')).toBeNull();
  });
  it('requests only a necessary directory when backups are enabled without an associated path', async () => {
    const { snapshot } = fixture();
    snapshot.projects[0].backupEnabled = true; snapshot.projects[0].installedPath = null;
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'preview_install') throw new Error('未记录安装路径，请明确选择软件目录');
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('打开安装程序').click(); await settle();
    expect(target.querySelector('dialog[open]')).not.toBeNull();
    expect(target.querySelector('dialog input[type="checkbox"]')).toBeNull();
    expect(mocks.dispatch.mock.calls.some(([action]) => action === 'install_download')).toBe(false);
  });
  it('checks only the selected software on explicit detection and does not invent success', async () => {
    const { snapshot, check } = fixture('unconfirmed');
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'verify_installations') return [check];
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('立即检测').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('verify_installations', { projectId: check.projectId });
    expect(target.querySelector('.toasts')?.textContent ?? target.textContent).toContain('保留原记录');
    expect(target.textContent).not.toContain('已验证安装版本并同步记录');
  });
  it('records the downloaded task target in one click without requesting the latest release', async () => {
    const { snapshot, check } = fixture('unconfirmed');
    mocks.dispatch.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'confirm_installation') {
        const project = snapshot.projects.find(item => item.id === check.projectId)!;
        project.installedVersion = check.expectedVersion; snapshot.installations![0] = { ...check, status: 'manual', source: 'manual' };
        return project;
      }
      throw new Error(`Unexpected action ${action}`);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    button('标记已安装 v6.5.0').click(); await settle();
    expect(mocks.dispatch).toHaveBeenCalledWith('confirm_installation', { id: check.id });
    expect(mocks.dispatch.mock.calls.some(([action]) => action === 'check_updates')).toBe(false);
    expect(target.querySelector('dialog')).toBeNull(); expect(snapshot.installations![0].status).toBe('manual');
  });
});
