import { beforeEach, describe, expect, it, vi } from 'vitest';
import { GithubCacheClient, GithubCacheSuperseded, GITHUB_CACHE_TTL } from '../src/lib/github-cache';
const { dispatchMock } = vi.hoisted(() => ({ dispatchMock: vi.fn() }));
vi.mock('../src/lib/bridge', () => ({ dispatch: dispatchMock }));
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
async function settle() { for (let i = 0; i < 30; i += 1) await Promise.resolve(); }
beforeEach(() => { dispatchMock.mockReset(); });
describe('assignment cache and bounded prefetch', () => {
  it('uses fresh validated current-session disk membership without a live read', async () => {
    const client = new GithubCacheClient(() => 2000); client.setSession('fixture', 3);
    const disk = { data: { listIds: ['one', 'two'] }, fresh: true, updatedAt: 1000, sessionRevision: 3 };
    dispatchMock.mockResolvedValue(disk);
    expect(await client.load('github_repository_lists', { repository: 'fixture/repo' }, { preferFreshCache: true })).toEqual(disk);
    expect(dispatchMock.mock.calls.map(([action]) => action)).toEqual(['github_cache_read']);
  });
  it.each(['stale', 'expired', 'revision', 'malformed'] as const)('does not trust %s disk membership', async (reason) => {
    const client = new GithubCacheClient(() => GITHUB_CACHE_TTL.lists + 2000); client.setSession('fixture', 3);
    const disk = { data: reason === 'malformed' ? { listIds: [9] } : { listIds: ['one'] }, fresh: reason !== 'stale', updatedAt: reason === 'expired' ? 1000 : GITHUB_CACHE_TTL.lists + 1000, sessionRevision: reason === 'revision' ? 2 : 3 };
    dispatchMock.mockImplementation((action) => action === 'github_cache_read' ? disk : { listIds: ['two'] });
    expect((await client.load('github_repository_lists', { repository: 'fixture/repo' }, { preferFreshCache: true })).data).toEqual({ listIds: ['two'] });
    expect(dispatchMock).toHaveBeenCalledWith('github_repository_lists', { repository: 'fixture/repo' });
  });
  it('limits membership work to two, deduplicates hover and fences queued work on account change', async () => {
    const client = new GithubCacheClient(); client.setSession('fixture', 3);
    const pending = [deferred<{ listIds: string[] }>(), deferred<{ listIds: string[] }>()];
    const started: string[] = [];
    dispatchMock.mockImplementation((action, parameters) => {
      if (action === 'github_cache_read') return null;
      started.push(parameters.repository); return pending[started.length - 1].promise;
    });
    for (let i = 0; i < 30; i += 1) client.prefetch('github_repository_lists', { repository: `fixture/r${i}` });
    client.prefetch('github_repository_lists', { repository: 'fixture/r0' }, true);
    await settle(); expect(started).toEqual(['fixture/r0', 'fixture/r1']);
    client.setSession('other', 4); pending.forEach((item) => item.resolve({ listIds: ['old'] }));
    await settle(); expect(started).toHaveLength(2);
    expect(client.isFresh('github_repository_lists', { repository: 'fixture/r0' })).toBe(false);
  });
  it('invalidates memberships and prevents old completion repopulating memory', async () => {
    const client = new GithubCacheClient(); client.setSession('fixture', 3);
    const old = deferred<{ listIds: string[] }>();
    dispatchMock.mockImplementation((action) => action === 'github_cache_read' ? null : old.promise);
    const result = client.load('github_repository_lists', { repository: 'fixture/repo' }, { preferFreshCache: true });
    await settle(); client.invalidate(['github_repository_lists']); old.resolve({ listIds: ['old'] });
    await expect(result).rejects.toBeInstanceOf(GithubCacheSuperseded);
    dispatchMock.mockImplementation((action) => action === 'github_cache_read' ? null : { listIds: ['new'] });
    expect((await client.load('github_repository_lists', { repository: 'fixture/repo' })).data).toEqual({ listIds: ['new'] });
  });
});
