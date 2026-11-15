import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import GithubFeed from '../../components/GithubFeed.svelte';
import { previewFeed } from './preview';
import { createPreviewGithubDetail } from '../github-preview';
import type { GithubFeedPage } from '../types';

const { dispatchMock } = vi.hoisted(() => ({ dispatchMock: vi.fn() }));
vi.mock('../bridge', () => ({ dispatch: dispatchMock }));
let target: HTMLDivElement;
let cleanup: (() => Promise<void>) | null;
const onopen = vi.fn();
const onnotice = vi.fn();
const onadd = vi.fn(async () => true);
const live = () => ({ ...previewFeed(), account: 'real-account', requestId: 'request-live' });
async function settle() { for (let index = 0; index < 12; index++) { await Promise.resolve(); flushSync(); } }
function deferred<T>() { let resolve!: (result: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
function button(label: string) { const result = [...target.querySelectorAll('button')].find((node) => node.textContent?.trim() === label); if (!result) throw new Error(`Missing button: ${label}`); return result; }
async function click(label: string) { button(label).click(); await settle(); }
function show(preview = false) { const instance = mount(GithubFeed, { target, props: { preview, onopen, onadd, onnotice } }); cleanup = async () => { await unmount(instance); }; flushSync(); }

beforeEach(() => {
  vi.useFakeTimers(); vi.clearAllMocks(); cleanup = null;
  target = document.createElement('div'); document.body.append(target);
  dispatchMock.mockImplementation(async (action: string) => { if (action === 'feed_status') return live(); if (action === 'github_repository') return createPreviewGithubDetail('tauri-apps/tauri', true); throw new Error(`Unexpected action: ${action}`); });
});
afterEach(async () => { await cleanup?.(); target.remove(); vi.clearAllTimers(); vi.useRealTimers(); });

describe('homepage feed UI (synthetic fixtures only)', () => {
  it('connects the independent website session without requiring gh login', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'feed_connect' ? { ...previewFeed('login'), loading: true } : previewFeed('login'));
    show(); await settle(); expect(target.textContent).toContain('首次使用需要再登录一次');
    await click('连接官网'); expect(dispatchMock).toHaveBeenCalledWith('feed_connect', {});
    expect(target.textContent).toContain('在官网窗口完成登录');
    expect(dispatchMock.mock.calls.some(([action]) => action === 'github_login')).toBe(false);
  });
  it('renders escaped plain text, opens original links and looks up repositories before adding', async () => {
    const page = live(); page.items = [{ ...page.items[0], summary: '<img src=x onerror=bad()>plain', body: '中'.repeat(200) }];
    dispatchMock.mockImplementation(async (action: string) => action === 'github_repository' ? createPreviewGithubDetail('tauri-apps/tauri', true) : page);
    show(); await settle(); expect(target.textContent).toContain('<img src=x onerror=bad()>plain'); expect(target.querySelector('.feed-copy img')).toBeNull();
    expect(target.querySelector('.feed-body')!.classList.contains('collapsed')).toBe(false);
    await click('查看原文'); expect(onopen).toHaveBeenCalledWith('https://github.com/tauri-apps/tauri/releases');
    await click('加入软件库'); expect(dispatchMock).toHaveBeenCalledWith('github_repository', { repository: 'tauri-apps/tauri' });
    expect(onadd).toHaveBeenCalledTimes(1); expect(button('已加入').disabled).toBe(true);
  });
  it('clears old account rows immediately on confirmed disconnect and ignores a stale poll', async () => {
    const late = deferred<GithubFeedPage>(); const logout = deferred<GithubFeedPage>(); let polls = 0;
    dispatchMock.mockImplementation(async (action: string) => action === 'feed_disconnect' ? logout.promise : ++polls === 1 ? live() : late.promise);
    show(); await settle(); await vi.advanceTimersByTimeAsync(5000); await settle();
    await click('退出网页'); expect(target.textContent).toContain('确认退出'); await click('确认退出');
    expect(target.querySelectorAll('.feed-row')).toHaveLength(0);
    late.resolve(live()); await settle(); expect(target.querySelectorAll('.feed-row')).toHaveLength(0);
    logout.resolve({ ...previewFeed('login'), sessionRevision: 2 }); await settle(); expect(target.textContent).not.toContain('@real-account');
  });
  it('ignores old-session poll responses and stops polling when unmounted', async () => {
    let polls = 0;
    dispatchMock.mockImplementation(async () => ++polls === 1 ? { ...live(), sessionRevision: 3 } : { ...live(), account: 'stale-account', sessionRevision: 1 });
    show(); await settle(); await vi.advanceTimersByTimeAsync(5000); await settle();
    expect(target.textContent).toContain('@real-account'); expect(target.textContent).not.toContain('@stale-account');
    await cleanup?.(); cleanup = null; const count = polls; await vi.advanceTimersByTimeAsync(10000); expect(polls).toBe(count);
  });
  it('does not downgrade real errors to demo data and can retry', async () => {
    dispatchMock.mockImplementation(async (action: string) => { if (action === 'feed_refresh') return live(); return { ...live(), items: [], error: 'Synthetic actual-reader failure' }; });
    show(); await settle(); expect(target.textContent).toContain('Synthetic actual-reader failure'); expect(target.querySelectorAll('.feed-row')).toHaveLength(0);
    expect(target.textContent).not.toContain('以下内容为演示'); await click('重试'); expect(target.querySelectorAll('.feed-row')).toHaveLength(3);
  });
  it('keeps pagination in webpage order and exposes content expansion', async () => {
    const page = live(); page.items[0].body = '长文本'.repeat(200);
    dispatchMock.mockImplementation(async (action: string) => action === 'feed_more' ? { ...page, hasMore: false, items: [...page.items, { ...page.items[0], id: 'last', summary: 'Last webpage item' }] } : page);
    show(); await settle(); expect(target.querySelector('.feed-body')!.classList.contains('collapsed')).toBe(true);
    await click('展开内容'); expect(target.querySelector('.feed-body')!.classList.contains('collapsed')).toBe(false);
    await click('加载更多'); expect(dispatchMock).toHaveBeenCalledWith('feed_more', {});
    expect(target.querySelectorAll('.feed-summary')[3].textContent).toBe('Last webpage item');
  });
  it('labels preview fixtures explicitly and never uses native commands in preview', async () => {
    show(true); await settle(); expect(target.textContent).toContain('以下内容为演示，不是真实账号动态');
    expect(dispatchMock).not.toHaveBeenCalled(); expect(button('刷新').disabled).toBe(true);
    const select = target.querySelector('select')!; select.value = 'error'; select.dispatchEvent(new Event('change', { bubbles: true })); await settle();
    expect(target.textContent).toContain('示例错误'); expect(dispatchMock).not.toHaveBeenCalled();
  });
});
