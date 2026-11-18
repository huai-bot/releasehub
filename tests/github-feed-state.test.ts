import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createFeedState } from '../src/lib/feed-state';
import { previewFeed } from '../src/lib/feed/preview';
import type { GithubFeedPage } from '../src/lib/types';
vi.mock('../src/lib/bridge', () => ({ dispatch: vi.fn() }));
const live = () => ({ ...previewFeed(), account: 'fixture-account' });
function deferred<T>() { let resolve!: (data: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
async function settle() { for (let index = 0; index < 16; index += 1) await Promise.resolve(); }
beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });
describe('shared website feed state', () => {
  it('shares one in-flight request and one timer between Home, account and feed; hidden consumers never fetch', async () => {
    const response = deferred<GithubFeedPage>(); const send = vi.fn(async () => response.promise); const state = createFeedState(send);
    const hidden = state.acquire(false); expect(send).not.toHaveBeenCalled();
    const home = state.acquire(); const account = state.acquire(); const feed = state.acquire();
    expect(send).toHaveBeenCalledTimes(1); response.resolve(live()); await settle();
    expect(get(state).page?.account).toBe('fixture-account'); expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(5000); await settle(); expect(send).toHaveBeenCalledTimes(2);
    home.release(); account.setActive(false); feed.release(); await vi.advanceTimersByTimeAsync(10000); expect(send).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0); account.release(); hidden.release();
  });
  it('preloads once while hidden, validates cached content and does not start hidden polling', async () => {
    const saved = { ...live(), connected: false, cached: true };
    const send = vi.fn(async (name: string) => name === 'feed_refresh' ? live() : saved);
    const state = createFeedState(send); const hidden = state.acquire(false);
    await state.preload(); await settle();
    expect(send.mock.calls.map(([name]) => name)).toEqual(['feed_status', 'feed_refresh']);
    expect(get(state).page?.connected).toBe(true); expect(vi.getTimerCount()).toBe(0);
    hidden.setActive(true); expect(send).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(5000); await settle();
    expect(send.mock.calls.filter(([name]) => name === 'feed_status')).toHaveLength(2);
    hidden.release();
  });
  it('keeps state on remount and validates restored content only once per session revision', async () => {
    const saved = { ...live(), connected: false, cached: true }; const send = vi.fn(async (_name: string) => saved); const state = createFeedState(send);
    const first = state.acquire(); await settle(); first.release();
    expect(send.mock.calls.filter(([name]) => name === 'feed_refresh')).toHaveLength(1);
    const second = state.acquire(); expect(get(state).page?.items).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(10000); await settle();
    expect(send.mock.calls.filter(([name]) => name === 'feed_refresh')).toHaveLength(1); second.release();
  });
  it('fences a pending status after disconnect, clears every consumer immediately, and resumes polling', async () => {
    const late = deferred<GithubFeedPage>(); const disconnect = deferred<GithubFeedPage>(); let reads = 0;
    const send = vi.fn(async (name: string) => name === 'feed_disconnect' ? disconnect.promise : ++reads === 1 ? live() : late.promise);
    const state = createFeedState(send); const consumer = state.acquire(); await settle();
    await vi.advanceTimersByTimeAsync(5000); await settle(); const operation = state.action('feed_disconnect');
    expect(get(state).page?.items).toHaveLength(0); late.resolve(live()); await settle(); expect(get(state).page?.account).toBeNull();
    disconnect.resolve({ ...previewFeed('login'), sessionRevision: 2 }); await operation; await settle();
    expect(get(state).page?.account).toBeNull(); expect(vi.getTimerCount()).toBe(1); consumer.release();
  });
  it('host account logout invalidates late website replies without dispatching another logout', async () => {
    const late = deferred<GithubFeedPage>(); const send = vi.fn(async () => late.promise); const state = createFeedState(send); const consumer = state.acquire();
    state.disconnectLocally(); late.resolve(live()); await settle(); expect(get(state).page).toBeNull(); expect(send).toHaveBeenCalledTimes(1); consumer.release();
  });
});
