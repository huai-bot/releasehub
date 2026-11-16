import { beforeEach, describe, expect, it, vi } from 'vitest';
import { GithubCacheClient, GithubCacheSuperseded, GITHUB_CACHE_TTL, githubCacheIdentity, githubCacheKey, readGithubCache } from './github-cache';
import { createPreviewGithubDetail, createPreviewGithubStatus } from './github-preview';
import type { GithubCacheEntry, GithubRepositoryPage } from './types';

const { dispatchMock } = vi.hoisted(() => ({ dispatchMock: vi.fn() }));
vi.mock('./bridge', () => ({ dispatch: dispatchMock }));
const parameters = { tab: 'explore', query: 'windows', page: 1 } as const;
const page = (name = 'demo/cached'): GithubRepositoryPage => ({ items: [createPreviewGithubDetail(name, true).repository], page: 1, hasMore: false, totalCount: 1 });
const disk = (data = page()): GithubCacheEntry<GithubRepositoryPage> => ({ data, updatedAt: 1000, fresh: true, sessionRevision: 3 });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: Error) => void; const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; }
async function settle() { for (let i = 0; i < 10; i += 1) await Promise.resolve(); }
beforeEach(() => dispatchMock.mockReset());

describe('typed machine-local GitHub cache', () => {
  it('separates identities and normalizes only parameter ordering', () => {
    const status = createPreviewGithubStatus('connected');
    expect(githubCacheIdentity(null)).toBe('anonymous');
    expect(githubCacheIdentity({ ...status, account: { ...status.account!, id: 123 } })).toBe('123');
    expect(githubCacheKey('github_repositories', parameters)).toBe(githubCacheKey('github_repositories', { page: 1, query: 'windows', tab: 'explore' }));
    expect(githubCacheKey('github_list_repositories', { listId: 'all', cursor: undefined })).toBe(githubCacheKey('github_list_repositories', { listId: 'all' }));
    expect(githubCacheKey('github_repositories', parameters)).not.toBe(githubCacheKey('github_repositories', { ...parameters, page: 2 }));
  });

  it('validates disk data and rejects another session revision', async () => {
    dispatchMock.mockResolvedValue(disk());
    expect(await readGithubCache('github_repositories', parameters, 3)).toEqual(disk());
    expect(await readGithubCache('github_repositories', parameters, 4)).toBeNull();
    dispatchMock.mockResolvedValue({ ...disk(), data: { items: [{ id: 1 }], page: 1, hasMore: false, totalCount: 1 } });
    expect(await readGithubCache('github_repositories', parameters, 3)).toBeNull();
    dispatchMock.mockResolvedValue({ ...disk(), updatedAt: NaN });
    expect(await readGithubCache('github_repositories', parameters, 3)).toBeNull();
    expect(dispatchMock).toHaveBeenCalledWith('github_cache_read', { operation: 'github_repositories', parameters });
  });

  it('shows even fresh disk data first, verifies once, then avoids network within TTL', async () => {
    let now = 20_000;
    const client = new GithubCacheClient(() => now); client.setSession('123', 3);
    const network = deferred<GithubRepositoryPage>(); const shown = vi.fn();
    dispatchMock.mockImplementation((action: string) => action === 'github_cache_read' ? disk() : network.promise);
    const loaded = client.load('github_repositories', parameters, { onCached: shown });
    await settle(); expect(shown).toHaveBeenCalledExactlyOnceWith(disk());
    expect(dispatchMock.mock.calls.map(([action]) => action)).toEqual(['github_cache_read', 'github_repositories']);
    network.resolve(page('demo/network')); const entry = await loaded;
    expect(entry.data.items[0].fullName).toBe('demo/network');
    const requests = dispatchMock.mock.calls.length;
    now += GITHUB_CACHE_TTL.search - 1;
    expect((await client.load('github_repositories', parameters)).data).toEqual(entry.data);
    expect(dispatchMock).toHaveBeenCalledTimes(requests);
    now += 1;
    await client.load('github_repositories', parameters);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_repositories')).toHaveLength(2);
  });

  it('deduplicates cache/network reads and notifies all waiting consumers', async () => {
    const client = new GithubCacheClient(); client.setSession('123', 3);
    const diskRead = deferred<GithubCacheEntry<GithubRepositoryPage>>(); const network = deferred<GithubRepositoryPage>();
    dispatchMock.mockImplementation((action: string) => action === 'github_cache_read' ? diskRead.promise : network.promise);
    const first = vi.fn(); const second = vi.fn();
    const a = client.load('github_repositories', parameters, { onCached: first });
    const b = client.load('github_repositories', parameters, { onCached: second });
    diskRead.resolve(disk()); await settle();
    expect(first).toHaveBeenCalledOnce(); expect(second).toHaveBeenCalledOnce();
    network.resolve(page()); await Promise.all([a, b]);
    expect(dispatchMock.mock.calls.map(([action]) => action)).toEqual(['github_cache_read', 'github_repositories']);
  });

  it('keeps cached content after network failure and retries without rereading disk', async () => {
    const client = new GithubCacheClient(); client.setSession('123', 3); const shown = vi.fn();
    dispatchMock.mockImplementation(async (action: string) => { if (action === 'github_cache_read') return disk(); throw new Error('offline'); });
    await expect(client.load('github_repositories', parameters, { onCached: shown })).rejects.toThrow('offline');
    expect(shown).toHaveBeenCalledWith(disk());
    dispatchMock.mockResolvedValue(page('demo/recovered'));
    expect((await client.load('github_repositories', parameters, { onCached: shown })).data.items[0].fullName).toBe('demo/recovered');
    expect(shown).toHaveBeenCalledTimes(2);
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_cache_read')).toHaveLength(1);
  });

  it('falls back to network on unsupported or corrupt cache', async () => {
    const client = new GithubCacheClient();
    dispatchMock.mockImplementation(async (action: string) => { if (action === 'github_cache_read') throw new Error('cache unavailable'); return page(); });
    expect((await client.load('github_repositories', parameters)).data).toEqual(page());
  });

  it('forced refresh bypasses disk/TTL and does not persist through browser storage', async () => {
    const local = vi.spyOn(Storage.prototype, 'setItem');
    const client = new GithubCacheClient(); dispatchMock.mockResolvedValue(page());
    await client.load('github_repositories', parameters, { force: true });
    await client.load('github_repositories', parameters, { force: true });
    expect(dispatchMock.mock.calls.map(([action]) => action)).toEqual(['github_repositories', 'github_repositories']);
    expect(local).not.toHaveBeenCalled(); local.mockRestore();
  });

  it('drops late network results on account change, logout, or cache clear', async () => {
    const client = new GithubCacheClient(); client.setSession('alice', 1);
    const late = deferred<GithubRepositoryPage>();
    dispatchMock.mockImplementation((action: string) => action === 'github_cache_read' ? null : late.promise);
    const pending = client.load('github_repositories', parameters); const rejected = expect(pending).rejects.toBeInstanceOf(GithubCacheSuperseded);
    await settle(); client.setSession('bob', 2); late.resolve(page('alice/private')); await rejected;
    dispatchMock.mockResolvedValue(page('bob/public'));
    expect((await client.load('github_repositories', parameters, { force: true })).data.items[0].fullName).toBe('bob/public');
    expect(client.isFresh('github_repositories', parameters)).toBe(true); client.clear();
    expect(client.isFresh('github_repositories', parameters)).toBe(false);
    expect(client.setSession('anonymous', 3)).toBe(true);
  });

  it('does not dispatch an old network request when the account changes during disk read', async () => {
    const client = new GithubCacheClient(); client.setSession('alice', 3);
    const late = deferred<GithubCacheEntry<GithubRepositoryPage>>(); dispatchMock.mockReturnValue(late.promise);
    const shown = vi.fn(); const pending = client.load('github_repositories', parameters, { onCached: shown });
    const rejected = expect(pending).rejects.toBeInstanceOf(GithubCacheSuperseded);
    client.setSession('bob', 4); late.resolve(disk()); await rejected;
    expect(shown).not.toHaveBeenCalled(); expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it('invalidates only mutated operations and ignores their late responses', async () => {
    const client = new GithubCacheClient(); const late = deferred<GithubRepositoryPage>();
    const activity = { items: [], page: 1, hasMore: false };
    dispatchMock.mockImplementation((action: string) => action === 'github_activity' ? activity : late.promise);
    await client.load('github_activity', { page: 1 }, { force: true });
    const pending = client.load('github_repositories', parameters, { force: true });
    const rejected = expect(pending).rejects.toBeInstanceOf(GithubCacheSuperseded);
    client.invalidate(['github_repositories']); late.resolve(page('demo/outdated')); await rejected;
    expect(client.isFresh('github_activity', { page: 1 })).toBe(true);
    await client.load('github_activity', { page: 1 });
    expect(dispatchMock.mock.calls.filter(([action]) => action === 'github_activity')).toHaveLength(1);
    expect(client.isFresh('github_repositories', parameters)).toBe(false);
  });

  it('reads disk and revalidates again in a new workspace after restart', async () => {
    dispatchMock.mockImplementation(async (action: string) => action === 'github_cache_read' ? disk() : page('demo/latest'));
    const first = new GithubCacheClient(); first.setSession('123', 3);
    await first.load('github_repositories', parameters);
    const restored = new GithubCacheClient(); restored.setSession('123', 3); const shown = vi.fn();
    await restored.load('github_repositories', parameters, { onCached: shown });
    expect(shown).toHaveBeenCalledExactlyOnceWith(disk());
    expect(dispatchMock.mock.calls.map(([action]) => action)).toEqual(['github_cache_read', 'github_repositories', 'github_cache_read', 'github_repositories']);
  });

  it.each([
    ['github_repositories', { tab: 'owned', query: '', page: 1 }, GITHUB_CACHE_TTL.owned, page()],
    ['github_list_repositories', { listId: 'all' }, GITHUB_CACHE_TTL.stars, { items: [], endCursor: null, hasMore: false, totalCount: 0 }],
    ['github_lists', {}, GITHUB_CACHE_TTL.lists, []],
    ['github_repository', { repository: 'demo/cache' }, GITHUB_CACHE_TTL.detail, createPreviewGithubDetail('demo/cache', true)],
    ['github_activity', { page: 1 }, GITHUB_CACHE_TTL.activity, { items: [], page: 1, hasMore: false }],
  ] as const)('applies the expected TTL for %s', async (operation, payload, ttl, result) => {
    let now = 10_000; const client = new GithubCacheClient(() => now); dispatchMock.mockResolvedValue(result);
    await client.load(operation, payload, { force: true });
    now += ttl - 1; expect(client.isFresh(operation, payload)).toBe(true);
    now += 1; expect(client.isFresh(operation, payload)).toBe(false);
  });
});
