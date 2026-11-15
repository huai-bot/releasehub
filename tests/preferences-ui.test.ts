import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import App from '../src/App.svelte';
import Sidebar from '../src/components/Sidebar.svelte';
import TitleBar from '../src/components/TitleBar.svelte';
import ProxySettings from '../src/components/ProxySettings.svelte';
import { createPreviewSnapshot } from '../src/lib/preview';
import { defaultNavigationOrder, moveNavigation, normalizeNavigation } from '../src/lib/navigation';
import type { ProxyTestResult, Settings } from '../src/lib/types';

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
  const item = [...target.querySelectorAll<HTMLButtonElement>('button')].find(item => item.getAttribute('aria-label') === label || item.textContent?.trim() === label);
  if (!item) throw new Error(`Missing button ${label}`);
  return item;
}
async function input(selector: string, value: string) {
  const item = target.querySelector<HTMLInputElement | HTMLSelectElement>(selector)!;
  item.value = value; item.dispatchEvent(new Event(item.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true })); await settle();
}
function submit() { target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); }
const success: ProxyTestResult = { ok: true, mode: 'manual', source: 'manual', target: 'https://api.github.com/', status: 200, latencyMs: 17, message: 'GitHub 连接正常' };

beforeEach(() => {
  vi.clearAllMocks(); cleanup = undefined;
  target = document.createElement('div'); document.body.append(target);
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) });
});
afterEach(async () => { await cleanup?.(); target.remove(); });

describe('startup and desktop navigation', () => {
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
    expect(target.querySelector('h1')?.textContent).toContain('软件库');
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

  it('uses native window controls and releases its resize listener', async () => {
    const instance = mount(TitleBar, { target, props: { title: 'GitHub' } });
    cleanup = async () => { await unmount(instance); }; await settle();
    button('最小化窗口').click(); button('最大化窗口').click(); button('关闭窗口').click(); await settle();
    expect(mocks.minimized).toHaveBeenCalledOnce(); expect(mocks.maximized).toHaveBeenCalledOnce(); expect(mocks.closed).toHaveBeenCalledOnce();
    expect(target.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true);
    await cleanup(); cleanup = undefined; expect(mocks.resizeCleanup).toHaveBeenCalledOnce();
  });
});

describe('network proxy preferences', () => {
  async function show(settings = { mode: 'system', url: '', username: '' } as Settings['proxy'], saved = false, save = vi.fn(async () => true)) {
    const ontest = vi.fn(async (_proxy: Settings['proxy'], _password?: string) => success);
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
