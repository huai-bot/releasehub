import { dispatch, type Actions } from './bridge';
import type { GithubCacheEntry, GithubCacheOperation, GithubStatus } from './types';

type Payload<O extends GithubCacheOperation> = Actions[O]['payload'];
type Result<O extends GithubCacheOperation> = Actions[O]['result'];
type Entry<O extends GithubCacheOperation> = GithubCacheEntry<Result<O>>;
type Stored = { entry: GithubCacheEntry; verifiedAt: number | null; ttl: number };

export const GITHUB_CACHE_TTL = { search: 600_000, owned: 300_000, stars: 300_000, lists: 300_000, detail: 3_600_000, activity: 120_000 } as const;

export function githubCacheIdentity(status: GithubStatus | null): string {
  return status?.account && status.connected ? `${status.account.id ?? status.account.login.toLowerCase()}` : 'anonymous';
}

export function githubCacheKey<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>): string {
  return `${operation}:${JSON.stringify(Object.fromEntries(Object.entries(parameters).filter(([, value]) => value !== undefined).sort(([a], [b]) => a.localeCompare(b))))}`;
}

function ttlFor<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>): number {
  if (operation === 'github_repository') return GITHUB_CACHE_TTL.detail;
  if (operation === 'github_activity') return GITHUB_CACHE_TTL.activity;
  if (operation === 'github_repositories') return 'tab' in parameters && parameters.tab === 'explore' ? GITHUB_CACHE_TTL.search : GITHUB_CACHE_TTL.owned;
  return GITHUB_CACHE_TTL.lists;
}

function record(value: unknown): value is Record<string, unknown> { return typeof value === 'object' && value !== null; }
function strings(value: unknown): value is string[] { return Array.isArray(value) && value.every((item) => typeof item === 'string'); }
function repository(value: unknown): boolean {
  return record(value) && typeof value.id === 'number' && ['name', 'fullName', 'owner', 'url', 'updatedAt', 'defaultBranch'].every((key) => typeof value[key] === 'string')
    && ['description', 'language', 'homepage'].every((key) => value[key] === null || typeof value[key] === 'string')
    && ['private', 'archived', 'fork', 'canAdmin'].every((key) => typeof value[key] === 'boolean')
    && ['stars', 'forks', 'openIssues'].every((key) => typeof value[key] === 'number')
    && (value.starred === null || typeof value.starred === 'boolean') && strings(value.topics);
}

/** Disk JSON is untrusted; never let a malformed cache replace a working view. */
export function isGithubCacheData<O extends GithubCacheOperation>(operation: O, data: unknown): data is Result<O> {
  if (operation === 'github_lists') return Array.isArray(data) && data.every((item) => record(item) && typeof item.id === 'string' && typeof item.name === 'string' && typeof item.url === 'string' && typeof item.itemCount === 'number' && (item.description === null || typeof item.description === 'string'));
  if (!record(data)) return false;
  if (operation === 'github_repository') return repository(data.repository) && typeof data.readme === 'string';
  if (operation === 'github_repository_lists') return strings(data.listIds);
  if (!Array.isArray(data.items) || typeof data.hasMore !== 'boolean') return false;
  if (operation === 'github_activity') return typeof data.page === 'number' && data.items.every((item) => record(item) && ['id', 'actor', 'repository', 'type', 'summary', 'url', 'createdAt'].every((key) => typeof item[key] === 'string') && (item.avatarUrl === null || typeof item.avatarUrl === 'string'));
  if (!data.items.every(repository)) return false;
  if (operation === 'github_list_repositories') return typeof data.totalCount === 'number' && (data.endCursor === null || typeof data.endCursor === 'string');
  return typeof data.page === 'number' && (data.totalCount === null || typeof data.totalCount === 'number');
}

export async function readGithubCache<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>, revision?: number): Promise<Entry<O> | null> {
  const entry = await dispatch('github_cache_read', { operation, parameters });
  if (!entry || !Number.isFinite(entry.updatedAt) || entry.updatedAt < 0 || typeof entry.fresh !== 'boolean' || !Number.isSafeInteger(entry.sessionRevision)
    || (revision !== undefined && entry.sessionRevision !== revision) || !isGithubCacheData(operation, entry.data)) return null;
  return { ...entry, data: entry.data };
}

export class GithubCacheSuperseded extends Error {
  constructor() { super('GitHub 账号或缓存已变更，忽略旧请求。'); }
}

/** Per-workspace memory only. Persistence belongs to the machine-local backend, never localStorage. */
export class GithubCacheClient {
  private generation = 0;
  private identity = 'anonymous';
  private revision: number | undefined;
  private memory = new Map<string, Stored>();
  private pending = new Map<string, Promise<GithubCacheEntry>>();
  private observers = new Map<string, Set<(entry: GithubCacheEntry) => void>>();
  private operationRevisions = new Map<GithubCacheOperation, number>();
  private prefetchQueue: Array<() => Promise<void>> = [];
  private prefetchKeys = new Set<string>();
  private prefetchActive = 0;
  constructor(private now: () => number = Date.now) {}

  setSession(identity: string, revision?: number): boolean {
    if (identity === this.identity && revision === this.revision) return false;
    this.identity = identity; this.revision = revision; this.clear(); return true;
  }

  clear(): void { this.generation += 1; this.memory.clear(); this.pending.clear(); this.observers.clear(); this.prefetchQueue = []; this.prefetchKeys.clear(); }

  /** Share one bounded queue across Stars batches, hover and detail preloads. */
  prefetch<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>, priority = false): void {
    if (this.isFresh(operation, parameters)) return;
    const generation = this.generation;
    const revision = this.operationRevisions.get(operation) ?? 0;
    const key = `${generation}:${revision}:${githubCacheKey(operation, parameters)}`;
    if (this.prefetchKeys.has(key)) return;
    this.prefetchKeys.add(key);
    const job = async () => {
      try {
        if (generation === this.generation && revision === (this.operationRevisions.get(operation) ?? 0)) {
          await this.load(operation, parameters, { preferFreshCache: true });
        }
      } catch { /* Background failures must not replace the view or the user's selection. */ }
      finally { this.prefetchKeys.delete(key); }
    };
    if (priority) this.prefetchQueue.unshift(job); else this.prefetchQueue.push(job);
    this.drainPrefetch();
  }

  private drainPrefetch(): void {
    while (this.prefetchActive < 2 && this.prefetchQueue.length) {
      const job = this.prefetchQueue.shift()!;
      this.prefetchActive += 1;
      void job().finally(() => { this.prefetchActive -= 1; this.drainPrefetch(); });
    }
  }

  isFresh<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>): boolean {
    const stored = this.memory.get(githubCacheKey(operation, parameters));
    return stored?.verifiedAt !== null && stored?.verifiedAt !== undefined && this.now() - stored.verifiedAt < stored.ttl;
  }

  /** Invalidate only affected operations; unrelated views and in-flight requests stay warm. */
  invalidate(operations: readonly GithubCacheOperation[]): void {
    for (const operation of operations) {
      this.operationRevisions.set(operation, (this.operationRevisions.get(operation) ?? 0) + 1);
      for (const key of new Set([...this.memory.keys(), ...this.pending.keys(), ...this.observers.keys()])) {
        if (key.startsWith(`${operation}:`)) { this.memory.delete(key); this.pending.delete(key); this.observers.delete(key); }
      }
    }
  }

  async load<O extends GithubCacheOperation>(operation: O, parameters: Payload<O>, options: { force?: boolean; preferFreshCache?: boolean; onCached?: (entry: Entry<O>) => void } = {}): Promise<Entry<O>> {
    const key = githubCacheKey(operation, parameters);
    const generation = this.generation;
    const operationRevision = this.operationRevisions.get(operation) ?? 0;
    const current = () => generation === this.generation && operationRevision === (this.operationRevisions.get(operation) ?? 0);
    const stored = this.memory.get(key);
    if (stored && options.onCached) options.onCached(stored.entry as Entry<O>);
    if (!options.force && stored?.verifiedAt !== null && stored?.verifiedAt !== undefined && this.now() - stored.verifiedAt < stored.ttl) return stored.entry as Entry<O>;
    const observer = options.onCached as ((entry: GithubCacheEntry) => void) | undefined;
    if (observer) {
      const observers = this.observers.get(key) ?? new Set(); observers.add(observer); this.observers.set(key, observers);
    }
    let request = this.pending.get(key);
    if (!request) {
      request = (async (): Promise<GithubCacheEntry> => {
        if (!stored && !options.force) {
          // Cache failure is not a network failure; fall through to the audited network action.
          let cached: Entry<O> | null = null;
          try { cached = await readGithubCache(operation, parameters, this.revision); } catch { /* Older hosts and cache I/O failures remain usable. */ }
          if (!current()) throw new GithubCacheSuperseded();
          if (cached) {
            const ttl = ttlFor(operation, parameters);
            const fresh = options.preferFreshCache && cached.fresh && this.now() - cached.updatedAt < ttl;
            this.memory.set(key, { entry: cached, verifiedAt: fresh ? cached.updatedAt : null, ttl });
            this.observers.get(key)?.forEach((notify) => notify(cached));
            // Editors and background preloads can trust a validated, current-session disk entry.
            // Browsing keeps its existing stale-while-revalidate policy.
            if (fresh) return cached;
          }
        }
        if (!current()) throw new GithubCacheSuperseded();
        // Even a fresh disk entry is verified once during this workspace lifetime.
        const data = await dispatch(operation, parameters);
        if (!current()) throw new GithubCacheSuperseded();
        const updatedAt = this.now();
        const entry = { data, updatedAt, fresh: true, sessionRevision: this.revision ?? 0 };
        this.memory.set(key, { entry, verifiedAt: updatedAt, ttl: ttlFor(operation, parameters) });
        return entry;
      })();
      this.pending.set(key, request);
      void request.finally(() => {
        if (this.pending.get(key) === request) { this.pending.delete(key); this.observers.delete(key); }
      }).catch(() => {});
    }
    try { return await request as Entry<O>; }
    finally { if (current() && observer) this.observers.get(key)?.delete(observer); }
  }
}
