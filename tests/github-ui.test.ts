import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Github from '../src/pages/Github.svelte';
import AiConnection from '../src/components/AiConnection.svelte';
import App from '../src/App.svelte';
import { createPreviewGithubDetail, createPreviewGithubStatus, createPreviewAgentIntegration } from '../src/lib/github-preview';
import { createPreviewSnapshot } from '../src/lib/preview';
import { previewFeed } from '../src/lib/feed/preview';
import { GITHUB_DEVICE_URL, githubAvatarUrl, isSubscribed, repositoryEditError } from '../src/lib/github';
import type { GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubStatus } from '../src/lib/types';

const { dispatchMock } = vi.hoisted(() => ({ dispatchMock: vi.fn() }));
vi.mock('../src/lib/bridge', () => ({ dispatch: dispatchMock, desktopAvailable: () => true, watchChanges: async () => () => {} }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  isMaximized: async () => false, onResized: async () => () => {}, setTheme: async () => {},
  minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {},
}) }));
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | null;
const onopen = vi.fn();
const oncopy = vi.fn();
const onnotice = vi.fn();
const onadd = vi.fn(async () => true);
const anonymous = () => createPreviewGithubStatus('anonymous');
const connected = () => createPreviewGithubStatus('connected');
const repo = (name: string, changes: Partial<GithubRepository> = {}) => ({ ...createPreviewGithubDetail(name, true).repository, ...changes });
const detail = (name: string, changes: Partial<GithubRepository> = {}): GithubRepositoryDetail => ({ repository: repo(name, changes), readme: '# Example\n\nSynthetic test data.' });
const page = (items: GithubRepository[], number = 1, more = false): GithubRepositoryPage => ({ items, page: number, hasMore: more, totalCount: items.length });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
async function settle() { for (let i = 0; i < 20; i += 1) { await Promise.resolve(); flushSync(); } }
function button(label: string) { const found = [...target.querySelectorAll('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label); if (!found) throw new Error(`Missing button: ${label}`); return found; }
async function click(label: string) { button(label).click(); await settle(); }
async function navigate(label: string) {
  const item = [...target.querySelectorAll<HTMLButtonElement>('nav button')].find((item) => item.querySelector('span')?.textContent === label);
  if (!item) throw new Error(`Missing navigation: ${label}`);
  item.click(); await settle();
}
async function input(selector: string, value: string) { const field = target.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)!; field.value = value; field.dispatchEvent(new Event('input', { bubbles: true })); await settle(); }
async function search(value: string) { await input('#github-search', value); target.querySelector('.github-search')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle(); }
function showGithub(mountTarget: HTMLElement = target) { const instance = mount(Github, { target: mountTarget, props: { projects: [], preview: false, onadd, onopen, oncopy, onnotice } }); cleanup = async () => { await unmount(instance); }; flushSync(); }
const base = (action: string): unknown => {
  if (action === 'github_status') return anonymous();
  if (action === 'github_repositories') return page([repo('demo/alpha')]);
  if (action === 'github_repository') return detail('demo/alpha');
  if (action === 'github_cache_read') return null;
  if (action === 'get_repository_notes') return [];
  throw new Error(`Unexpected action: ${action}`);
};

beforeEach(() => {
  vi.clearAllMocks(); cleanup = null;
  dispatchMock.mockImplementation(async (action: string) => base(action));
  target = document.createElement('div'); document.body.append(target);
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.setAttribute('open', ''); } });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value() { this.removeAttribute('open'); } });
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) });
});
afterEach(async () => { await cleanup?.(); target.remove(); vi.useRealTimers(); });

describe('GitHub scoped UI, synthetic accounts only', () => {
  it('allows anonymous search/detail but gates private tabs and unsafe README links', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'github_repository' ? { ...detail('demo/alpha', { starred: null }), readme: '<script>bad()</script><img src=x onerror=bad()>\n\n[Safe](https://github.com/demo/alpha/issues) [Unsafe](javascript:bad()) [Other](https://example.com)' } : base(action));
    showGithub(); await settle();
    expect(dispatchMock).toHaveBeenCalledWith('github_repositories', { tab: 'explore', query: 'windows', page: 1 });
    await click('查看 demo/alpha 详情');
    expect(button('登录后可 Star').disabled).toBe(true);
    expect(target.querySelector('.github-detail script, .github-detail img')).toBeNull();
    const links = [...target.querySelectorAll<HTMLAnchorElement>('.github-detail a[data-external-url]')];
    expect(links).toHaveLength(1); links[0].click(); await settle();
    expect(onopen).toHaveBeenCalledWith('https://github.com/demo/alpha/issues');
    await click('我的仓库');
    expect(target.textContent).toContain('登录后查看自己的仓库');
    expect(dispatchMock.mock.calls.some(([action, data]) => action === 'github_repositories' && data.tab === 'owned')).toBe(false);
  });

  it('opens only the fixed device page, copies the code, and stops polling after success', async () => {
    vi.useFakeTimers(); let statusCalls = 0;
    const waiting = { ...createPreviewGithubStatus('waiting'), login: { ...createPreviewGithubStatus('waiting').login!, verificationUri: 'https://example.invalid/not-github' } };
    dispatchMock.mockImplementation(async (action: string) => action === 'github_login' ? waiting : action === 'github_status' ? (++statusCalls === 1 ? anonymous() : connected()) : base(action));
    showGithub(); await settle(); await click('登录 GitHub');
    expect(onopen).not.toHaveBeenCalled();
    await click('复制短码'); expect(oncopy).toHaveBeenCalledWith('DEMO-1234');
    await click('打开 GitHub 授权页'); expect(onopen).toHaveBeenCalledWith(GITHUB_DEVICE_URL);
    await vi.advanceTimersByTimeAsync(2000); await settle();
    expect(target.textContent).toContain('@releasehub-demo');
    const count = statusCalls; await vi.advanceTimersByTimeAsync(6000); await settle();
    expect(statusCalls).toBe(count); expect(vi.getTimerCount()).toBe(0);
  });

  it('cancels login and ignores a late poll response; unmount stops further status requests', async () => {
    vi.useFakeTimers(); let calls = 0; const late = deferred<GithubStatus>();
    dispatchMock.mockImplementation(async (action: string) => action === 'github_login' ? createPreviewGithubStatus('waiting') : action === 'github_cancel_login' ? anonymous() : action === 'github_status' ? (++calls === 1 ? anonymous() : late.promise) : base(action));
    showGithub(); await settle(); await click('登录 GitHub');
    await vi.advanceTimersByTimeAsync(2000); await settle(); expect(calls).toBe(2);
    await click('取消登录'); expect(dispatchMock).toHaveBeenCalledWith('github_cancel_login', {});
    late.resolve(connected()); await settle();
    expect(target.textContent).not.toContain('@releasehub-demo');
    // Svelte also schedules a zero-delay click-event cleanup; assert polling behavior, not its timers.
    const callsAfterCancel = calls; await vi.advanceTimersByTimeAsync(6000); await settle();
    expect(calls).toBe(callsAfterCancel); expect(target.textContent).not.toContain('@releasehub-demo');
    await click('登录 GitHub'); expect(button('取消登录').disabled).toBe(false);
    const callsBeforeUnmount = calls; await cleanup?.(); cleanup = null;
    await vi.advanceTimersByTimeAsync(6000); await settle();
    expect(calls).toBe(callsBeforeUnmount); expect(target.textContent).toBe('');
  });

  it('keeps the latest search when older requests or initial account detection arrive later', async () => {
    const old = deferred<GithubRepositoryPage>(); const initialStatus = deferred<GithubStatus>();
    dispatchMock.mockImplementation(async (action: string, payload: { query?: string }) => action === 'github_status' ? initialStatus.promise : action === 'github_repositories' ? payload.query === 'old' ? old.promise : page([repo(`demo/${payload.query}`)]) : base(action));
    showGithub(); await settle(); await search('old'); await search('new');
    old.resolve(page([repo('demo/stale')])); initialStatus.resolve(connected()); await settle();
    expect(target.querySelector<HTMLInputElement>('#github-search')!.value).toBe('new');
    expect(target.textContent).toContain('demo/new'); expect(target.textContent).not.toContain('demo/stale');
  });

  it('ignores old tab/detail responses and clears private content immediately on logout', async () => {
    const oldTab = deferred<{ items: GithubRepository[]; endCursor: null; hasMore: boolean; totalCount: number }>(); const oldDetail = deferred<GithubRepositoryDetail>(); const logout = deferred<GithubStatus>();
    dispatchMock.mockImplementation(async (action: string, payload: { tab?: string; repository?: string }) => {
      if (action === 'github_status') return connected();
      if (action === 'github_logout') return logout.promise;
      if (action === 'github_lists') return [];
      if (action === 'github_list_repositories') return oldTab.promise;
      if (action === 'github_repositories') return page([repo('demo/private-a', { id: 1, private: true }), repo('demo/private-b', { id: 2, private: true })]);
      if (action === 'github_repository') return payload.repository === 'demo/private-a' ? oldDetail.promise : detail('demo/private-b', { private: true });
      return base(action);
    });
    showGithub(); await settle(); await click('Stars'); await click('我的仓库');
    oldTab.resolve({ items: [repo('demo/stale-star')], endCursor: null, hasMore: false, totalCount: 1 }); await settle(); expect(target.textContent).not.toContain('stale-star');
    await click('查看 demo/private-a 详情'); await click('查看 demo/private-b 详情');
    oldDetail.resolve(detail('demo/private-a')); await settle();
    expect(target.querySelector('.github-detail')!.textContent).toContain('demo/private-b');
    expect(target.querySelector('.github-detail')!.textContent).not.toContain('demo/private-a');
    button('仅断开追新').click(); await settle(); expect(target.textContent).not.toContain('demo/private');
    logout.resolve(anonymous()); await settle(); expect(target.textContent).toContain('登录后查看自己的仓库');
  });

  it('filters owned repositories only on this page and confirms Star and admin edits explicitly', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'github_status' ? connected() : action === 'github_repository' ? detail('demo/alpha', { starred: true, canAdmin: true }) : action === 'github_set_star' ? { starred: false } : action === 'github_update_repository' ? repo('demo/alpha', { description: 'Changed', homepage: 'https://example.com' }) : base(action));
    showGithub(); await settle(); await click('我的仓库');
    const before = dispatchMock.mock.calls.filter(([action]) => action === 'github_repositories').length;
    await input('#github-search', 'not-on-page'); expect(target.textContent).toContain('本页没有匹配的仓库');
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_repositories')).toHaveLength(before);
    await click('清空本页筛选'); await click('查看 demo/alpha 详情'); await click('取消 Star');
    expect(dispatchMock).toHaveBeenCalledWith('github_set_star', { repository: 'demo/alpha', starred: false, confirmed: true });
    await click('修改说明与主页'); expect(button('确认保存到 GitHub').disabled).toBe(true);
    await input('dialog textarea', 'Changed'); await input('dialog input[type="url"]', 'https://example.com');
    target.querySelector<HTMLInputElement>('dialog input[type="checkbox"]')!.click(); await settle();
    target.querySelector('dialog form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
    expect(dispatchMock).toHaveBeenCalledWith('github_update_repository', { repository: 'demo/alpha', description: 'Changed', homepage: 'https://example.com', confirmed: true });
    expect(target.querySelector('dialog')).toBeNull();
  });

  it('shows errors with retry and preserves anonymous browsing when CLI is missing', async () => {
    let failed = true;
    dispatchMock.mockImplementation(async (action: string) => { if (action === 'github_status') return createPreviewGithubStatus('unavailable'); if (action === 'github_repositories' && failed) throw new Error('Synthetic network failure'); return base(action); });
    showGithub(); await settle(); expect(target.textContent).toContain('没有找到 GitHub 登录组件');
    expect(button('登录 GitHub').disabled).toBe(true); expect(target.textContent).toContain('Synthetic network failure');
    failed = false; await click('重试这一页'); expect(target.textContent).toContain('demo/alpha');
  });

  it('restores each tab query, page, filter, detail and scroll without refetching fresh data', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: { tab?: string; query?: string; page?: number; repository?: string }) => {
      if (action === 'github_status') return connected();
      if (action === 'github_repositories') return page([repo(payload.tab === 'owned' ? 'demo/owned' : `demo/${payload.query}-${payload.page}`)], payload.page, true);
      if (action === 'github_repository') return detail(payload.repository!);
      return base(action);
    });
    const main = document.createElement('main'); target.append(main); showGithub(main); await settle();
    await search('tools'); await click('下一页'); await click('查看 demo/tools-2 详情'); main.scrollTop = 325;
    const searchReads = dispatchMock.mock.calls.filter(([action, data]) => action === 'github_repositories' && data.tab === 'explore').length;
    const detailReads = dispatchMock.mock.calls.filter(([action]) => action === 'github_repository').length;
    await click('我的仓库'); await input('#github-search', 'owned'); main.scrollTop = 72;
    await click('搜索仓库');
    expect(target.querySelector<HTMLInputElement>('#github-search')?.value).toBe('tools');
    expect(target.textContent).toContain('demo/tools-2'); expect(target.querySelector('.github-pagination')?.textContent).toContain('第 2 页');
    expect(target.querySelector('.github-detail')?.textContent).toContain('demo/tools-2'); expect(main.scrollTop).toBe(325);
    expect(dispatchMock.mock.calls.filter(([action, data]) => action === 'github_repositories' && data.tab === 'explore')).toHaveLength(searchReads);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_repository')).toHaveLength(detailReads);
    await click('我的仓库');
    expect(target.querySelector<HTMLInputElement>('#github-search')?.value).toBe('owned'); expect(main.scrollTop).toBe(72);
    expect(dispatchMock.mock.calls.filter(([action, data]) => action === 'github_repositories' && data.tab === 'owned')).toHaveLength(1);
  });

  it('shows disk content after remount before even a fresh entry finishes its network verification', async () => {
    showGithub(); await settle(); await cleanup?.(); cleanup = null;
    const network = deferred<GithubRepositoryPage>();
    dispatchMock.mockImplementation(async (action: string) => {
      if (action === 'github_cache_read') return { data: page([repo('demo/from-disk')]), updatedAt: Date.now(), fresh: true, sessionRevision: 0 };
      if (action === 'github_repositories') return network.promise;
      return base(action);
    });
    dispatchMock.mockClear(); showGithub(); await settle();
    expect(target.textContent).toContain('demo/from-disk'); expect(target.textContent).toContain('正在后台更新');
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_repositories')).toHaveLength(1);
    network.resolve(page([repo('demo/from-network')])); await settle();
    expect(target.textContent).toContain('demo/from-network'); expect(target.textContent).not.toContain('demo/from-disk');
  });

  it('keeps old rows while paging and preserves them when a forced refresh fails', async () => {
    const next = deferred<GithubRepositoryPage>(); let fail = false;
    dispatchMock.mockImplementation(async (action: string, payload: { page?: number }) => {
      if (action === 'github_repositories') { if (fail) throw new Error('模拟后台更新失败'); return payload.page === 2 ? next.promise : page([repo('demo/page-one')], 1, true); }
      return base(action);
    });
    showGithub(); await settle(); await click('下一页');
    expect(target.textContent).toContain('demo/page-one'); expect(target.querySelector('.skeleton-list')).toBeNull();
    next.resolve(page([repo('demo/page-two')], 2)); await settle();
    fail = true; await click('刷新仓库列表');
    expect(target.textContent).toContain('demo/page-two'); expect(target.textContent).toContain('模拟后台更新失败');
    expect(target.querySelector('.github-pagination')?.textContent).toContain('第 2 页');
  });

  it('does not let an old account request refill its view after switching accounts', async () => {
    let account = connected(); const old = deferred<GithubRepositoryPage>();
    dispatchMock.mockImplementation(async (action: string, payload: { tab?: string }) => {
      if (action === 'github_status') return account;
      if (action === 'github_repositories') return payload.tab === 'owned' && account.account!.login === 'releasehub-demo' ? old.promise : page([repo(`${account.account!.login}/current`)]);
      return base(action);
    });
    showGithub(); await settle(); await click('我的仓库');
    account = { ...connected(), account: { ...connected().account!, login: 'new-account' }, sessionRevision: 2 };
    await click('刷新 GitHub 账号状态'); old.resolve(page([repo('old-account/private', { private: true })])); await settle();
    expect(target.textContent).toContain('new-account/current'); expect(target.textContent).not.toContain('old-account/private');
    await click('搜索仓库'); await click('我的仓库'); expect(target.textContent).not.toContain('old-account/private');
  });

  it('invalidates pending content on cache clear without discarding visible rows', async () => {
    const stale = deferred<GithubRepositoryPage>(); let reads = 0;
    dispatchMock.mockImplementation(async (action: string) => action === 'github_repositories' ? ++reads === 2 ? stale.promise : page([repo(reads === 1 ? 'demo/kept' : 'demo/new-read')]) : base(action));
    showGithub(); await settle(); await click('刷新仓库列表');
    window.dispatchEvent(new Event('releasehub:cache-cleared')); await settle();
    expect(target.textContent).toContain('demo/kept');
    stale.resolve(page([repo('demo/stale-refill')])); await settle();
    expect(target.textContent).toContain('demo/kept'); expect(target.textContent).not.toContain('demo/stale-refill');
    await click('刷新仓库列表'); expect(target.textContent).toContain('demo/new-read'); expect(reads).toBe(3);
  });

  it('keeps unsaved repository fields during background list refresh', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'github_status' ? connected() : action === 'github_repository' ? detail('demo/alpha', { canAdmin: true }) : base(action));
    showGithub(); await settle(); await click('查看 demo/alpha 详情'); await click('修改说明与主页');
    await input('dialog textarea', '尚未保存的说明'); await input('dialog input[type="url"]', 'https://example.com/draft');
    await click('刷新仓库列表');
    expect(target.querySelector<HTMLTextAreaElement>('dialog textarea')?.value).toBe('尚未保存的说明');
    expect(target.querySelector<HTMLInputElement>('dialog input[type="url"]')?.value).toBe('https://example.com/draft');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_update_repository')).toBe(false);
  });

  it('labels a recovered account as unverified and requests private scope only explicitly', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'github_status' ? { ...connected(), provider: 'github-oauth', verified: false, scopes: ['read:user', 'public_repo', 'offline_access'], error: '暂未连通' } : action === 'github_login' ? createPreviewGithubStatus('waiting') : base(action));
    showGithub(); await settle(); expect(target.textContent).toContain('已恢复本机账号，当前尚未验证');
    expect(target.textContent).toContain('repo 权限允许读写公开与私有仓库');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_login')).toBe(false);
    await click('授权私有仓库（repo）'); expect(dispatchMock).toHaveBeenCalledWith('github_login', { privateRepositories: true });
  });

  describe('AiConnection information sharing', () => {
    it('starts off and copies all five platform formats plus the same-executable CLI command', async () => {
      const snapshot = createPreviewSnapshot(); const config = createPreviewAgentIntegration();
      const onintegration = vi.fn(async () => config); const onsave = vi.fn();
      let instance = mount(AiConnection, { target, props: { settings: snapshot.settings, runtime: snapshot.runtime, busy: false, preview: false, onsave, onintegration, oncopy } });
      cleanup = async () => { await unmount(instance); }; await settle();
      expect(onintegration).not.toHaveBeenCalled(); expect(target.querySelector('[role="switch"]')!.getAttribute('aria-checked')).toBe('false');
      await click('开启软件资料连接'); expect(onsave).toHaveBeenLastCalledWith({ agentEnabled: true });
      await cleanup();
      instance = mount(AiConnection, { target, props: { settings: { ...snapshot.settings, agentEnabled: true }, runtime: { ...snapshot.runtime, agent: { ...snapshot.runtime.agent, enabled: true } }, busy: false, preview: false, onsave, onintegration, oncopy } }); await settle();
      expect(onintegration).toHaveBeenCalledOnce(); expect(button('通用 JSON').getAttribute('aria-pressed')).toBe('true');
      for (const [label, value] of [['通用 JSON', config.genericConfig], ['Claude', config.claudeConfig], ['Codex', config.codexConfig], ['Cursor', config.cursorConfig], ['VS Code', config.vscodeConfig]]) {
        await click(label); await click('复制配置'); expect(oncopy).toHaveBeenLastCalledWith(value);
        expect(target.querySelector<HTMLTextAreaElement>('#mcp-client-config')!.value).toBe(value);
      }
      target.querySelector<HTMLDetailsElement>('.connection-detail')!.open = true;
      await click('复制查询命令'); expect(oncopy).toHaveBeenLastCalledWith(`${config.cliCommand} list --json`);
      expect(target.textContent).toContain('纯云端连接无法直接访问这台电脑');
      expect(target.querySelector('input[type="password"]')).toBeNull(); expect(target.textContent).not.toContain('重新生成密钥');
    });

    it('retries unavailable configuration and validates the local port before saving', async () => {
      const snapshot = createPreviewSnapshot(); const onsave = vi.fn();
      const onintegration = vi.fn().mockRejectedValueOnce(new Error('连接未就绪，请重新读取。')).mockResolvedValue(createPreviewAgentIntegration());
      const instance = mount(AiConnection, { target, props: { settings: { ...snapshot.settings, agentEnabled: true }, runtime: { ...snapshot.runtime, agent: { ...snapshot.runtime.agent, enabled: true } }, busy: false, preview: false, onsave, onintegration, oncopy } });
      cleanup = async () => { await unmount(instance); }; await settle();
      expect(target.querySelector('[role="alert"]')!.textContent).toContain('连接未就绪');
      await click('重新读取'); expect(onintegration).toHaveBeenCalledTimes(2); expect(button('复制配置').disabled).toBe(false);
      target.querySelector<HTMLDetailsElement>('.connection-advanced')!.open = true;
      await input('#ai-agent-port', '80'); target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
      expect(onsave).not.toHaveBeenCalled(); expect(target.querySelector('#agent-port-error')!.textContent).toContain('1024 到 65535');
      await input('#ai-agent-port', '47832'); target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
      expect(onsave).toHaveBeenCalledWith({ agentPort: 47832 }); expect(target.querySelector('#agent-port-error')).toBeNull();
    });

    it('keeps preview controls inert and drops a configuration response after unmount', async () => {
      const snapshot = createPreviewSnapshot(); const onintegration = vi.fn(async () => createPreviewAgentIntegration()); const onsave = vi.fn();
      let instance = mount(AiConnection, { target, props: { settings: snapshot.settings, runtime: snapshot.runtime, busy: false, preview: true, onsave, onintegration, oncopy } });
      cleanup = async () => { await unmount(instance); }; await settle();
      expect(button('开启软件资料连接').disabled).toBe(true); expect(button('应用').disabled).toBe(true);
      await click('查看配置示例'); expect(onintegration).not.toHaveBeenCalled();
      expect(button('复制配置').disabled).toBe(true); expect(target.querySelector<HTMLTextAreaElement>('#mcp-client-config')!.value).toContain('releasehub-mcp.exe');
      await cleanup(); const late = deferred<ReturnType<typeof createPreviewAgentIntegration>>();
      instance = mount(AiConnection, { target, props: { settings: { ...snapshot.settings, agentEnabled: true }, runtime: { ...snapshot.runtime, agent: { ...snapshot.runtime.agent, enabled: true } }, busy: false, preview: false, onsave, onintegration: () => late.promise, oncopy } }); await settle();
      expect(target.querySelector('[role="status"]')!.textContent).toContain('正在准备');
      await cleanup(); cleanup = null; late.resolve(createPreviewAgentIntegration()); await settle();
      expect(target.textContent).toBe(''); expect(oncopy).not.toHaveBeenCalled();
    });
  });

  it('retains the visited Stars workspace, filters, details and loaded pages without another request', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupGithubTab = 'starred';
    const alpha = repo('demo/alpha', { id: 1 }); const beta = repo('demo/beta', { id: 2 });
    dispatchMock.mockImplementation(async (action: string, payload: { cursor?: string }) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'github_status') return connected();
      if (action === 'get_repository_notes' || action === 'github_lists') return [];
      if (action === 'github_list_repositories') return { items: payload.cursor ? [beta] : [alpha], endCursor: payload.cursor ? null : 'next', hasMore: !payload.cursor, totalCount: 2 };
      return base(action);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    expect(dispatchMock.mock.calls.some(([action]) => action.startsWith('github_'))).toBe(false);
    await navigate('GitHub'); await click('继续加载 Stars'); await input('#github-search', 'alpha');
    target.querySelector<HTMLInputElement>('input[aria-label="选择 demo/alpha"]')!.click(); await settle();
    await click('查看 demo/alpha 详情');
    const results = target.querySelector('#github-results');
    const workspace = target.querySelector<HTMLElement>('#main-content')!; workspace.scrollTop = 260;
    const requests = dispatchMock.mock.calls.length;
    await navigate('资源库');
    expect(workspace.hidden).toBe(true); expect(target.querySelector('#main-content')).not.toBe(workspace);
    await navigate('GitHub');
    expect(dispatchMock.mock.calls).toHaveLength(requests);
    expect(target.querySelector('#github-results')).toBe(results);
    expect(target.querySelector('#main-content')).toBe(workspace); expect(workspace.hidden).toBe(false); expect(workspace.scrollTop).toBe(260);
    expect(target.querySelector('#github-tab-starred')?.getAttribute('aria-selected')).toBe('true');
    expect(target.querySelector<HTMLInputElement>('#github-search')?.value).toBe('alpha');
    expect(target.querySelector<HTMLInputElement>('input[aria-label="选择 demo/alpha"]')?.checked).toBe(true);
    expect(target.querySelector('.github-detail')?.textContent).toContain('demo/alpha');
    expect(workspace.textContent).toContain('已加载 2 批 · 2 / 2 个');
    const reads = dispatchMock.mock.calls.filter(([action]) => action === 'github_list_repositories').length;
    await click('刷新仓库列表');
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_list_repositories')).toHaveLength(reads + 1);
    expect(workspace.textContent).toContain('已加载 1 批');
  });

  it('clears retained account content on logout and accepts a changed account on refresh', async () => {
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'github'; snapshot.settings.startupGithubTab = 'owned';
    let account = connected(); const logout = deferred<GithubStatus>();
    dispatchMock.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'github_status') return account;
      if (action === 'get_repository_notes') return [];
      if (action === 'github_repositories') return page([repo(`${account.account!.login}/private`, { private: true })]);
      if (action === 'github_logout') return logout.promise;
      return base(action);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    await navigate('资源库'); await navigate('GitHub');
    expect(target.textContent).toContain('releasehub-demo/private');
    account = { ...connected(), account: { ...connected().account!, login: 'other-demo' } };
    await click('刷新 GitHub 账号状态');
    expect(target.textContent).not.toContain('releasehub-demo/private'); expect(target.textContent).toContain('other-demo/private');
    button('仅断开追新').click(); await settle();
    expect(target.textContent).not.toContain('other-demo/private');
    logout.resolve(anonymous()); await settle(); await navigate('资源库'); await navigate('GitHub');
    expect(target.textContent).not.toContain('other-demo/private'); expect(target.textContent).toContain('登录后查看自己的仓库');
  });

  it('retains homepage feed content and pauses status polling while another workspace is visible', async () => {
    vi.useFakeTimers();
    const snapshot = createPreviewSnapshot(); snapshot.settings.startupPage = 'github'; snapshot.settings.startupGithubTab = 'feed';
    dispatchMock.mockImplementation(async (action: string) => {
      if (action === 'snapshot') return snapshot;
      if (action === 'github_status') return connected();
      if (action === 'get_repository_notes') return [];
      if (action === 'feed_status') return previewFeed();
      return base(action);
    });
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    const feed = target.querySelector('.feed-list'); expect(feed).not.toBeNull();
    const reads = () => dispatchMock.mock.calls.filter(([action]) => action === 'feed_status').length;
    const before = reads(); await navigate('资源库'); await vi.advanceTimersByTimeAsync(15000); await settle();
    expect(reads()).toBe(before);
    await navigate('GitHub'); expect(target.querySelector('.feed-list')).toBe(feed); expect(reads()).toBe(before);
    expect(target.querySelector('#main-content')?.textContent).not.toContain('正在读取官网动态');
    await vi.advanceTimersByTimeAsync(5000); await settle(); expect(reads()).toBe(before + 1);
  });

  it('adds a public repository via add_project without downloads and has no normal task-creation UI', async () => {
    const snapshot = createPreviewSnapshot();
    dispatchMock.mockImplementation(async (action: string) => action === 'snapshot' ? snapshot : action === 'add_project' ? { ...snapshot.projects[0], id: 'new-project', owner: 'demo', repo: 'alpha' } : base(action));
    const instance = mount(App, { target }); cleanup = async () => { await unmount(instance); }; await settle();
    expect(target.textContent).not.toContain('Agent 任务'); expect(target.textContent).not.toContain('创建编译任务');
    const nav = [...target.querySelectorAll<HTMLButtonElement>('nav button')].find((item) => item.querySelector('span')?.textContent === 'GitHub')!; nav.click(); await settle();
    await click('加入资源库');
    expect(dispatchMock).toHaveBeenCalledWith('add_project', { input: { repository: 'demo/alpha', channel: 'stable', preference: 'auto' } });
    expect(dispatchMock.mock.calls.some(([action]) => ['check_updates', 'download_asset', 'install_download', 'create_build_task', 'reveal_agent_connection', 'rotate_agent_token'].includes(action))).toBe(false);
  });
});

it('checks safe avatar URLs, local duplicates and metadata input without credentials', () => {
  expect(githubAvatarUrl('https://avatars.githubusercontent.com/u/1')).toBe('https://avatars.githubusercontent.com/u/1');
  expect(githubAvatarUrl('https://example.com/avatar')).toBeNull(); expect(githubAvatarUrl('javascript:alert(1)')).toBeNull();
  expect(isSubscribed('DEMO/Alpha', [{ owner: 'demo', repo: 'alpha' }])).toBe(true);
  expect(repositoryEditError('x'.repeat(351), '')).toBeTruthy(); expect(repositoryEditError('ok', 'javascript:alert(1)')).toBeTruthy();
  expect(repositoryEditError('ok', 'https://example.com')).toBeNull();
});
