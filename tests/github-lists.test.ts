import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Github from '../src/pages/Github.svelte';
import { feedState } from '../src/lib/feed-state';
import { appendGithubRepositories, filterGithubPage } from '../src/lib/github';
import { createPreviewGithubDetail, createPreviewGithubStatus } from '../src/lib/github-preview';
import type { GithubListRepositoryPage, GithubRepository, GithubUserList, RepositoryNote } from '../src/lib/types';

const { dispatchMock } = vi.hoisted(() => ({ dispatchMock: vi.fn() }));
vi.mock('../src/lib/bridge', () => ({ dispatch: dispatchMock }));
const onadd = vi.fn(async () => true);
const onopen = vi.fn();
const oncopy = vi.fn();
const onnotice = vi.fn();
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | null;
let notes: RepositoryNote[];
let catalog: GithubUserList[];
const repo = (id: number, name: string): GithubRepository => ({ ...createPreviewGithubDetail(name, true).repository, id, starred: true });
const alpha = repo(1, 'demo/alpha');
const beta = repo(2, 'demo/beta');
const page = (items: GithubRepository[], endCursor: string | null = null, totalCount = items.length): GithubListRepositoryPage => ({ items, endCursor, hasMore: endCursor !== null, totalCount });
const list = (id: string, name: string): GithubUserList => ({ id, name, description: null, url: `https://github.com/stars/demo/lists/${id}`, itemCount: 1 });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
async function settle() { for (let i = 0; i < 16; i += 1) { await Promise.resolve(); flushSync(); } }
function button(label: string): HTMLButtonElement {
  const found = [...target.querySelectorAll('button')].find((item) => item.textContent?.trim() === label || item.getAttribute('aria-label') === label);
  if (!found) throw new Error(`Missing button: ${label}`);
  return found;
}
async function click(label: string) { button(label).click(); await settle(); }
async function input(selector: string, value: string) { const field = target.querySelector<HTMLInputElement>(selector)!; field.value = value; field.dispatchEvent(new Event('input', { bubbles: true })); await settle(); }
async function submit(selector: string) { target.querySelector(selector)!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle(); }
function show(initialTab: 'explore' | 'starred' | 'feed' = 'starred') {
  const instance = mount(Github, { target, props: { initialTab, projects: [], preview: false, onadd, onopen, oncopy, onnotice } });
  cleanup = async () => { await unmount(instance); }; flushSync();
}
function base(action: string, payload: Record<string, unknown> = {}): unknown {
  switch (action) {
    case 'github_status': return createPreviewGithubStatus('connected');
    case 'github_cache_read': return null;
    case 'get_repository_notes': return notes;
    case 'github_lists': return catalog;
    case 'github_list_repositories': return page([alpha]);
    case 'github_repositories': return { items: [alpha], page: 1, hasMore: false, totalCount: 1 };
    case 'github_repository': return { repository: alpha, readme: '# Synthetic fixture' };
    case 'github_repository_lists': return { listIds: ['list-a', 'list-b'] };
    case 'github_assign_lists': return { updated: 1 };
    case 'github_save_list': {
      const saved = { id: String(payload.id ?? 'list-new'), name: String(payload.name), description: String(payload.description), url: 'https://github.com/stars/demo/lists/new', itemCount: 0 };
      catalog = [...catalog.filter((item) => item.id !== saved.id), saved]; return saved;
    }
    case 'github_delete_list': catalog = catalog.filter((item) => item.id !== payload.id); return null;
    case 'save_repository_note': {
      const saved = { repository: String(payload.repository).toLowerCase(), name: String(payload.name), description: String(payload.description), updatedAt: '2026-01-01T00:00:00Z' };
      notes = notes.filter((note) => note.repository !== saved.repository);
      if (!saved.name && !saved.description) return null;
      notes = [...notes, saved]; return saved;
    }
    case 'feed_status': return { connected: false, loading: false, account: null, items: [], hasMore: false, lastUpdated: null, error: null, sourceUrl: 'https://github.com/', requestId: 'test', sessionRevision: 0 };
    default: throw new Error(`Unexpected action: ${action}`);
  }
}
beforeEach(() => {
  vi.clearAllMocks(); cleanup = null; feedState.clear(); notes = []; catalog = [list('list-a', '桌面工具'), list('list-b', '收藏')];
  dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => base(action, payload));
  target = document.createElement('div'); document.body.append(target);
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.setAttribute('open', ''); } });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value() { this.removeAttribute('open'); } });
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) });
});
afterEach(async () => { await cleanup?.(); target.remove(); vi.restoreAllMocks(); });

describe('GitHub Lists, loaded Stars and local notes', () => {
  it('extends filtering across cursor pages, including local notes, without calling global search', async () => {
    notes = [{ repository: 'demo/beta', name: '剪辑助手', description: '视频', updatedAt: '' }];
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_list_repositories' ? payload.cursor ? page([beta], null, 2) : page([alpha], 'next', 2) : base(action, payload));
    show(); await settle();
    expect(dispatchMock).toHaveBeenCalledWith('github_list_repositories', { listId: 'all' });
    await input('#github-search', '剪辑');
    expect(target.textContent).toContain('已加载的 Stars 中没有匹配结果');
    await click('继续加载 Stars');
    expect(dispatchMock).toHaveBeenCalledWith('github_list_repositories', { listId: 'all', cursor: 'next' });
    expect(target.querySelector('.repo-title')?.textContent).toContain('剪辑助手');
    expect(target.querySelector('.original-name')?.textContent).toContain('demo/beta');
    expect(target.textContent).toContain('已加载 2 批 · 2 / 2 个');
    await input('#github-search', 'alpha');
    expect(button('查看 demo/alpha 详情')).toBeDefined();
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_repositories')).toBe(false);
  });

  it('ignores late category pages and offers another page when unlisted has no match yet', async () => {
    const late = deferred<GithubListRepositoryPage>();
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_list_repositories' ? payload.listId === 'all' ? late.promise : page([], 'next', 1) : base(action, payload));
    show(); await settle(); await click('未分类');
    late.resolve(page([repo(7, 'demo/stale')])); await settle();
    expect(target.textContent).not.toContain('demo/stale');
    expect(target.textContent).toContain('这批 Stars 没有符合分类的仓库');
    expect(button('继续加载 Stars').disabled).toBe(false);
  });

  it('retains loaded rows and search when loading the next page fails', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => {
      if (action === 'github_list_repositories') { if (payload.cursor) throw new Error('模拟网络失败'); return page([alpha], 'next', 2); }
      return base(action, payload);
    });
    show(); await settle(); await input('#github-search', 'alpha'); await click('继续加载 Stars');
    expect(button('查看 demo/alpha 详情')).toBeDefined();
    expect(target.textContent).toContain('模拟网络失败');
    expect(button('重试下一批')).toBeDefined();
  });

  it('requires an explicit click before batch add and sends add rather than replace', async () => {
    show(); await settle();
    target.querySelector<HTMLInputElement>('input[aria-label="选择 demo/alpha"]')!.click(); await settle();
    await click('添加到分类');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_assign_lists')).toBe(false);
    target.querySelector<HTMLInputElement>('.assignment-options input')!.click(); await settle();
    await submit('.assignment-editor form');
    expect(dispatchMock).toHaveBeenCalledWith('github_assign_lists', { repositories: ['demo/alpha'], listIds: ['list-a'], mode: 'add', confirmed: true });
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_lists').length).toBeGreaterThan(1);
  });

  it('reads every current membership before enabling replacement and preserves checked categories', async () => {
    const late = deferred<{ listIds: string[] }>();
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_repository_lists' ? late.promise : base(action, payload));
    show(); await settle(); await click('设置 demo/alpha 分类');
    expect(button('确认同步分类').disabled).toBe(true);
    late.resolve({ listIds: ['list-a', 'list-b'] }); await settle();
    expect([...target.querySelectorAll<HTMLInputElement>('.assignment-options input')].every((checkbox) => checkbox.checked)).toBe(true);
    target.querySelector<HTMLInputElement>('.assignment-options input')!.click(); await settle();
    await submit('.assignment-editor form');
    expect(dispatchMock).toHaveBeenCalledWith('github_assign_lists', { repositories: ['demo/alpha'], listIds: ['list-b'], mode: 'replace', confirmed: true });
  });

  it('reuses fresh assignment snapshots and refreshes expired changes made on GitHub', async () => {
    const now = Date.now();
    const clock = vi.spyOn(Date, 'now').mockReturnValue(now);
    show(); await settle();
    const catalogReads = dispatchMock.mock.calls.filter(([action]) => action === 'github_lists').length;
    const membershipReads = dispatchMock.mock.calls.filter(([action]) => action === 'github_repository_lists').length;
    await click('设置 demo/alpha 分类');
    expect([...target.querySelectorAll<HTMLInputElement>('.assignment-options input')].map((checkbox) => checkbox.checked)).toEqual([true, true]);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_lists')).toHaveLength(catalogReads);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_repository_lists')).toHaveLength(membershipReads);
    catalog = [...catalog, list('list-new', '官网新增分类')];
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_repository_lists' ? { listIds: ['list-a', 'list-new'] } : base(action, payload));
    clock.mockReturnValue(now + 300_001);
    await click('关闭分类整理');
    await click('设置 demo/alpha 分类');
    expect([...target.querySelectorAll<HTMLInputElement>('.assignment-options input')].map((checkbox) => checkbox.checked)).toEqual([true, false, true]);
    await submit('.assignment-editor form');
    expect(dispatchMock).toHaveBeenCalledWith('github_assign_lists', { repositories: ['demo/alpha'], listIds: ['list-a', 'list-new'], mode: 'replace', confirmed: true });
  });

  it('does not enable replacement when memberships refer to a newly changed catalog', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_repository_lists' ? { listIds: ['list-not-yet-read'] } : base(action, payload));
    show(); await settle(); await click('设置 demo/alpha 分类');
    expect(button('确认同步分类').disabled).toBe(true);
    expect(target.textContent).toContain('请重新读取归属');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_assign_lists')).toBe(false);
  });

  it('creates, renames and deletes a category only through its explicit controls', async () => {
    show(); await settle(); await click('新建分类');
    await input('.list-editor input', '媒体'); await submit('.list-editor');
    expect(dispatchMock).toHaveBeenCalledWith('github_save_list', { name: '媒体', description: '', confirmed: true });
    const category = [...target.querySelectorAll<HTMLButtonElement>('.list-options button')].find((item) => item.textContent?.startsWith('媒体'))!;
    category.click(); await settle(); await click('改名与说明');
    await input('.list-editor input', '音视频'); await submit('.list-editor');
    expect(dispatchMock).toHaveBeenCalledWith('github_save_list', { id: 'list-new', name: '音视频', description: '', confirmed: true });
    await click('删除分类');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_delete_list')).toBe(false);
    await click('确认删除分类');
    expect(dispatchMock).toHaveBeenCalledWith('github_delete_list', { id: 'list-new', confirmed: true });
  });

  it('saves notes without a subscription and shows original identity in the row and detail', async () => {
    show(); await settle(); await click('编辑 demo/alpha 本地备注');
    await input('dialog input[data-initial-focus]', '快捷启动');
    await input('dialog input[placeholder="记录用途或下次查看的线索"]', '工具箱');
    await submit('dialog form');
    expect(dispatchMock).toHaveBeenCalledWith('save_repository_note', { repository: 'demo/alpha', name: '快捷启动', description: '工具箱' });
    expect(onadd).not.toHaveBeenCalled();
    expect(target.querySelector('.repo-title')?.textContent).toContain('快捷启动');
    expect(target.querySelector('.original-name')?.textContent).toContain('demo/alpha');
    await click('查看 demo/alpha 详情');
    expect(target.querySelector('.github-detail h2')?.textContent).toBe('快捷启动');
    await click('编辑本地备注');
    await input('dialog input[data-initial-focus]', '');
    await input('dialog input[placeholder="记录用途或下次查看的线索"]', '');
    await submit('dialog form');
    expect(target.querySelector('.github-detail h2')?.textContent).toBe('demo/alpha');
  });

  it('keeps an edited note open after a save error', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => { if (action === 'save_repository_note') throw new Error('模拟保存失败'); return base(action, payload); });
    show(); await settle(); await click('编辑 demo/alpha 本地备注');
    await input('dialog input[data-initial-focus]', '保留输入'); await submit('dialog form');
    expect(target.textContent).toContain('模拟保存失败');
    expect(target.querySelector<HTMLInputElement>('dialog input[data-initial-focus]')?.value).toBe('保留输入');
  });

  it('retains category, loaded cursor batches, filter and selection across internal tabs', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_list_repositories' ? payload.listId === 'list-a' ? payload.cursor ? page([beta], null, 2) : page([alpha], 'next', 2) : page([alpha]) : base(action, payload));
    show(); await settle();
    const category = [...target.querySelectorAll<HTMLButtonElement>('.list-options button')].find((item) => item.textContent?.startsWith('桌面工具'))!;
    category.click(); await settle(); await click('继续加载 Stars'); await input('#github-search', 'beta');
    target.querySelector<HTMLInputElement>('input[aria-label="选择 demo/beta"]')!.click(); await settle();
    const reads = dispatchMock.mock.calls.filter(([action]) => action === 'github_list_repositories').length;
    await click('搜索仓库'); await click('Stars');
    expect(target.querySelector<HTMLInputElement>('#github-search')?.value).toBe('beta');
    expect(target.querySelector('.list-options button.active')?.textContent).toContain('桌面工具');
    expect(target.textContent).toContain('已加载 2 批 · 2 / 2 个');
    expect(target.querySelector<HTMLInputElement>('input[aria-label="选择 demo/beta"]')?.checked).toBe(true);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_list_repositories')).toHaveLength(reads);
  });

  it('preserves unsaved category text when the catalog refreshes in the background', async () => {
    show(); await settle(); await click('新建分类'); await input('.list-editor input', '待整理');
    catalog = [...catalog, list('new-from-web', '官网新增')]; await click('刷新分类');
    expect(target.querySelector<HTMLInputElement>('.list-editor input')?.value).toBe('待整理');
    expect(target.textContent).toContain('官网新增');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_save_list')).toBe(false);
  });

  it('requests the optional list scope only from the authorization action', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_login' ? createPreviewGithubStatus('waiting') : base(action, payload));
    show(); await settle();
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_login')).toBe(false);
    await click('授权分类管理');
    expect(dispatchMock).toHaveBeenCalledWith('github_login', { lists: true });
  });

  it('shows the independent homepage feed without a CLI account and does not reset the chosen tab', async () => {
    dispatchMock.mockImplementation(async (action: string, payload: Record<string, unknown>) => action === 'github_status' ? createPreviewGithubStatus('anonymous') : base(action, payload));
    show('feed'); await settle();
    expect(dispatchMock).toHaveBeenCalledWith('feed_status', {});
    expect(target.querySelector('#github-tab-feed')?.getAttribute('aria-selected')).toBe('true');
    await click('搜索仓库'); await click('刷新 GitHub 账号状态');
    expect(target.querySelector('#github-tab-explore')?.getAttribute('aria-selected')).toBe('true');
  });
});

describe('loaded repository helpers', () => {
  it('deduplicates cursor overlap and filters multiple terms over original fields plus notes', () => {
    const items = appendGithubRepositories([alpha], [{ ...alpha, stars: 9 }, beta]);
    expect(items).toHaveLength(2); expect(items[0].stars).toBe(9);
    expect(filterGithubPage(items, 'DEMO 视频', [{ repository: 'DEMO/BETA', name: '视频助手', description: '', updatedAt: '' }])).toEqual([beta]);
  });
});
