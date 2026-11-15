import extractor from './dom-extractor.js?raw';
import receiver from './webview-receiver.js?raw';
import { runInNewContext } from 'node:vm';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const card = (number: number) => `<article id="feed-item-${number}"><h3>Activity ${number}</h3><a href="/sample/project-${number}">sample/project-${number}</a></article>`;
interface Sent { command: string; args: { payload: { requestId: string; windowToken: string; documentId: string; sequence: number; status: string; account: string | null; items: Array<{ summary: string }>; hasMore: boolean; error: string | null } } }
function boot(options: { url?: string; frame?: boolean; html?: string } = {}) {
  const doc = new DOMParser().parseFromString(`<head><meta name="user-login" content="first-user"></head><body><main>${options.html ?? card(0)}</main></body>`, 'text/html');
  const location = new URL(options.url ?? 'https://github.com/dashboard-feed');
  const sent: Sent[] = [];
  const events = new Map<string, Array<() => void>>();
  const invoke = vi.fn(async (command: string, args: Sent['args']) => { sent.push({ command, args }); });
  const win: { top?: unknown; location: URL; __TAURI_INTERNALS__: { invoke: typeof invoke }; __releasehubFeedReader?: { request: (id: string, action: string) => void } } = { location, __TAURI_INTERNALS__: { invoke } };
  win.top = options.frame ? {} : win;
  const history = { pushState(_state: unknown, _unused: string, url?: string) { if (url) location.href = new URL(url, location).href; }, replaceState(_state: unknown, _unused: string, url?: string) { if (url) location.href = new URL(url, location).href; } };
  runInNewContext(`${extractor}\n${receiver}\nstartReleaseHubFeed({requestId:'request-one',windowToken:'private-window-token'});`, {
    window: win, document: doc, location, history, Date, URL, Reflect, MutationObserver,
    crypto: { randomUUID: () => 'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa' }, setTimeout, clearTimeout, setInterval, clearInterval,
    addEventListener: (name: string, callback: () => void) => { events.set(name, [...(events.get(name) ?? []), callback]); },
  });
  return { doc, sent, win, history, emit: (name: string) => events.get(name)?.forEach((callback) => callback()) };
}
async function changes() { await Promise.resolve(); await Promise.resolve(); }

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe('isolated website receiver protocol', () => {
  it('only invokes feed_receive and transports plain snapshot fields with request/document identity', async () => {
    const reader = boot(); await changes();
    expect(reader.sent).toHaveLength(1); expect(reader.sent[0].command).toBe('feed_receive');
    expect(reader.sent[0].args.payload).toMatchObject({ requestId: 'request-one', windowToken: 'private-window-token', sequence: 1, account: 'first-user', status: 'ready' });
    expect(reader.sent[0].args.payload).not.toHaveProperty('more');
    expect(Object.keys(reader.win.__releasehubFeedReader!)).toEqual(['request']);
    expect(Object.isFrozen(reader.win.__releasehubFeedReader)).toBe(true);
  });
  it('does not start on an external origin or a child frame', () => {
    expect(boot({ url: 'https://evil.test/dashboard-feed' }).sent).toHaveLength(0);
    expect(boot({ frame: true }).sent).toHaveLength(0);
  });
  it('debounces mutations and deduplicates unchanged snapshots', async () => {
    const reader = boot(); await changes();
    for (let index = 0; index < 20; index++) reader.doc.querySelector('h3')!.textContent = `Changed ${index}`;
    await changes(); await vi.advanceTimersByTimeAsync(599); expect(reader.sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1); expect(reader.sent).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(21000); expect(reader.sent).toHaveLength(2);
  });
  it('does not starve under continuous mutation', async () => {
    const reader = boot(); await changes();
    for (let index = 0; index < 5; index++) {
      reader.doc.querySelector('h3')!.textContent = `Continuous ${index}`;
      await changes(); await vi.advanceTimersByTimeAsync(500);
    }
    expect(reader.sent.length).toBeGreaterThan(1);
  });
  it('clicks only the observed pagination control and keeps the new request id', async () => {
    const reader = boot({ html: `${card(0)}<button>Load more</button>` }); await changes();
    reader.doc.querySelector('button')!.addEventListener('click', () => reader.doc.querySelector('button')!.insertAdjacentHTML('beforebegin', card(1)));
    reader.win.__releasehubFeedReader!.request('request-two', 'more'); await changes();
    const last = reader.sent.at(-1)!.args.payload;
    expect(last.requestId).toBe('request-two'); expect(last.status).toBe('ready'); expect(last.items).toHaveLength(2);
    expect(last.sequence).toBeGreaterThan(reader.sent[0].args.payload.sequence);
  });
  it('reports stalled pagination as an error instead of pretending there are new items', async () => {
    const reader = boot({ html: `${card(0)}<button>Load more</button>` }); await changes();
    reader.win.__releasehubFeedReader!.request('request-two', 'more'); await changes();
    expect(reader.sent.at(-1)!.args.payload.status).toBe('loading');
    await vi.advanceTimersByTimeAsync(15000);
    expect(reader.sent.at(-1)!.args.payload.status).toBe('error');
    expect(reader.sent.at(-1)!.args.payload.error).toContain('尚未加载出更多');
  });
  it('clears old card data while an account meta change is ahead of the page content', async () => {
    const reader = boot(); await changes();
    reader.doc.querySelector('meta')!.setAttribute('content', 'second-user');
    await changes(); await vi.advanceTimersByTimeAsync(600);
    expect(reader.sent.at(-1)!.args.payload).toMatchObject({ account: 'second-user', status: 'loading', items: [] });
    reader.doc.querySelector('main')!.innerHTML = card(1);
    await changes(); await vi.advanceTimersByTimeAsync(600);
    expect(reader.sent.at(-1)!.args.payload).toMatchObject({ account: 'second-user', status: 'ready' });
    expect(reader.sent.at(-1)!.args.payload.items[0].summary).toBe('Activity 1');
  });
  it('resumes observing after a back-forward cache restore and reports history navigation', async () => {
    const reader = boot(); await changes(); reader.emit('pagehide');
    reader.doc.querySelector('h3')!.textContent = 'Restored'; await changes(); await vi.advanceTimersByTimeAsync(1000);
    expect(reader.sent).toHaveLength(1);
    reader.emit('pageshow'); await changes(); expect(reader.sent.at(-1)!.args.payload.items[0].summary).toBe('Restored');
    reader.doc.querySelector('h3')!.textContent = 'After restore'; await changes(); await vi.advanceTimersByTimeAsync(600);
    expect(reader.sent.at(-1)!.args.payload.items[0].summary).toBe('After restore');
    reader.history.pushState({}, '', '/sample/project'); await vi.advanceTimersByTimeAsync(600);
    expect(reader.sent.at(-1)!.args.payload.status).toBe('other');
  });
});
