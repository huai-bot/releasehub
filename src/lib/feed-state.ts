import { writable } from 'svelte/store';
import { dispatch } from './bridge';
import type { GithubFeedPage } from './types';
import { readableError } from './ui';

export type FeedAction = 'feed_connect' | 'feed_refresh' | 'feed_more' | 'feed_disconnect';
export type FeedSnapshot = { page: GithubFeedPage | null; pending: FeedAction | null; initialLoading: boolean; transportError: string };
type FeedDispatch = (action: FeedAction | 'feed_status', payload: Record<string, never>) => Promise<GithubFeedPage>;

/** One website state and one polling owner for Home, account panel and feed.
 * Subscribers alone do not launch requests; only visible consumers acquire it.
 */
export function createFeedState(send: FeedDispatch = dispatch) {
  const initial = (): FeedSnapshot => ({ page: null, pending: null, initialLoading: true, transportError: '' });
  let current = initial();
  const store = writable<FeedSnapshot>(current);
  const clients = new Map<symbol, boolean>();
  const validated = new Set<number>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let sequence = 0;
  let statusFlight: Promise<void> | null = null;
  let actionFlight: Promise<GithubFeedPage | null> | null = null;
  const visible = () => [...clients.values()].some(Boolean);
  function publish(update: Partial<FeedSnapshot>) { current = { ...current, ...update }; store.set(current); }
  function stop() { if (timer !== undefined) clearTimeout(timer); timer = undefined; }
  function schedule() {
    stop();
    if (!visible()) return;
    timer = setTimeout(() => { timer = undefined; void refreshStatus(); }, current.page?.loading || current.pending === 'feed_connect' ? 1200 : 5000);
  }
  function accept(page: GithubFeedPage) {
    if (current.page && page.sessionRevision < current.page.sessionRevision) return;
    publish({ page, transportError: '' });
  }
  async function refreshStatus(background = false) {
    if ((!background && !visible()) || current.pending) { schedule(); return; }
    if (statusFlight) return statusFlight;
    stop();
    const epoch = generation; const request = ++sequence;
    statusFlight = (async () => {
      let validate = false;
      try {
        const page = await send('feed_status', {});
        if (epoch !== generation || request !== sequence) return;
        accept(page);
        // Saved cards or a remembered website login are validated once per
        // session revision in the background; neither is shown as connected.
        if ((page.cached || page.remembered) && !page.connected && !page.loading && !validated.has(page.sessionRevision)) {
          validated.add(page.sessionRevision); validate = true;
        }
      } catch (error) {
        if (epoch === generation && request === sequence) publish({ transportError: readableError(error) });
      } finally {
        if (epoch === generation && request === sequence) { publish({ initialLoading: false }); schedule(); }
      }
      if (validate && (background || visible()) && epoch === generation && request === sequence) void action('feed_refresh');
    })();
    const flight = statusFlight;
    try { await flight; } finally { if (statusFlight === flight) statusFlight = null; }
  }
  async function action(name: FeedAction): Promise<GithubFeedPage | null> {
    if (actionFlight || (current.page?.loading && name === 'feed_more')) return null;
    stop(); const epoch = generation; const request = ++sequence;
    statusFlight = null; // Existing status may finish, but no longer owns polling.
    publish({ pending: name, transportError: '' });
    if (name === 'feed_disconnect') {
      // Fence old status replies and clear private cards in every consumer now.
      const page = current.page;
      if (page) publish({ page: { ...page, account: null, connected: false, cached: false, loading: false, items: [], hasMore: false, lastUpdated: null } });
    }
    actionFlight = (async () => {
      try {
        const page = await send(name, {});
        if (epoch !== generation || request !== sequence) return null;
        accept(page); return page;
      } catch (error) {
        if (epoch === generation && request === sequence) publish({ transportError: readableError(error) });
        return null;
      } finally {
        if (epoch === generation && request === sequence) { publish({ pending: null, initialLoading: false }); schedule(); }
      }
    })();
    const flight = actionFlight;
    try { return await flight; } finally { if (actionFlight === flight) actionFlight = null; }
  }
  function clear() {
    generation += 1; sequence += 1; stop(); statusFlight = null; actionFlight = null; validated.clear();
    current = initial(); store.set(current);
    if (visible()) void refreshStatus();
  }
  function disconnectLocally() {
    generation += 1; sequence += 1; stop(); statusFlight = null; actionFlight = null;
    const page = current.page;
    publish({ pending: null, transportError: '', initialLoading: false, page: page ? { ...page, account: null, connected: false, cached: false, loading: false, items: [], hasMore: false, lastUpdated: null } : null });
    schedule();
  }
  function acquire(active = true) {
    const id = Symbol('feed-consumer'); const wasVisible = visible(); clients.set(id, active);
    if (active && !wasVisible) { if (!current.page && current.initialLoading) void refreshStatus(); else schedule(); }
    return {
      setActive(next: boolean) {
        if (!clients.has(id) || clients.get(id) === next) return;
        const before = visible(); clients.set(id, next);
        if (!visible()) stop();
        else if (!before) { if (!current.page && current.initialLoading) void refreshStatus(); else schedule(); }
      },
      release() { clients.delete(id); if (!visible()) stop(); },
    };
  }
  return { subscribe: store.subscribe, acquire, action, refreshStatus, preload: () => refreshStatus(true), clear, disconnectLocally };
}

export const feedState = createFeedState();
if (typeof window !== 'undefined') {
  window.addEventListener('releasehub:cache-cleared', () => feedState.clear());
  window.addEventListener('releasehub:github-logout', () => feedState.disconnectLocally());
}
