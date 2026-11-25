<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import { feedState } from '../lib/feed-state';
  import { GITHUB_DEVICE_URL, appendGithubRepositories, filterGithubPage, findRepositoryNote, isSubscribed, repositoryEditError } from '../lib/github';
  import { GithubCacheClient, GithubCacheSuperseded, githubCacheIdentity } from '../lib/github-cache';
  import type { GithubActivityPage, GithubListRepositoryPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus, GithubUserList, Project, RepositoryNote } from '../lib/types';
  import { formatDate, readableError, type Tone } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import GithubAvatar from '../components/GithubAvatar.svelte';
  import GithubDetail from '../components/GithubDetail.svelte';
  import GithubFeed from '../components/GithubFeed.svelte';
  import type { GithubPreviewScenario } from '../lib/github-preview';

  let { projects, repositoryNotes, preview, active = true, preload = false, initialTab = 'explore', onadd, onopen, onoriginal, oncopy, onnotice, onnotes }: {
    projects: Project[]; repositoryNotes?: RepositoryNote[]; preview: boolean; active?: boolean; preload?: boolean; initialTab?: Tab;
    onadd: (repository: GithubRepository) => Promise<boolean>;
    onopen: (url: string) => void; oncopy: (text: string) => void; onnotice: (message: string, tone?: Tone) => void;
    onoriginal?: (url: string) => void;
    onnotes?: (notes: RepositoryNote[]) => void;
  } = $props();
  type Tab = GithubRepositoryTab | 'activity' | 'feed';
  const tabs = [
    { id: 'explore', label: '搜索仓库', icon: 'search' },
    { id: 'owned', label: '我的仓库', icon: 'user' },
    { id: 'starred', label: 'Stars', icon: 'star' },
    { id: 'feed', label: '首页动态', icon: 'home' },
    { id: 'activity', label: '关注动态', icon: 'activity' },
  ] as const;
  // The initial navigation target is consumed once, not on snapshot refreshes.
  let tab = $state<Tab>(untrack(() => initialTab));
  let searchText = $state(untrack(() => initialTab === 'explore' ? 'windows' : ''));
  let searchQuery = $state('windows');
  let page = $state(1);
  let status = $state<GithubStatus | null>(null);
  let statusLoading = $state(true);
  let statusError = $state('');
  let authBusy = $state(false);
  let repositories = $state<GithubRepositoryPage | null>(null);
  let activity = $state<GithubActivityPage | null>(null);
  let contentLoading = $state(false);
  let contentError = $state('');
  let selectedName = $state<string | null>(null);
  let detail = $state<GithubRepositoryDetail | null>(null);
  let detailLoading = $state(false);
  let detailError = $state('');
  let pendingRepositories = $state(new Set<string>());
  let addingRepositories = $state(new Set<string>());
  let editing = $state<GithubRepository | null>(null);
  let description = $state('');
  let homepage = $state('');
  let editConfirmed = $state(false);
  let editBusy = $state(false);
  let editError = $state('');
  let previewScenario = $state<GithubPreviewScenario>('connected');
  let detailRegion = $state<HTMLDivElement>();
  let previousFocus: HTMLElement | null = null;
  let linkBack = $state<(() => Promise<void>) | null>(null);
  let destroyed = false;
  let sessionRevision = 0;
  let statusRequest = 0;
  let listRequest = 0;
  let detailRequest = 0;
  let loginTimer: ReturnType<typeof setTimeout> | undefined;
  let lists = $state<GithubUserList[]>([]);
  let listsLoading = $state(false);
  let listsError = $state('');
  let listFilter = $state('all');
  let stars = $state<GithubListRepositoryPage | null>(null);
  let starPage = $state(1);
  let checkedRepositories = $state(new Set<string>());
  let editingList = $state<{ id?: string; name: string; description: string } | null>(null);
  let listDelete = $state<GithubUserList | null>(null);
  let listBusy = $state(false);
  let listEditError = $state('');
  let assignment = $state<{ repositories: string[]; mode: 'add' | 'remove' | 'replace'; listIds: string[] } | null>(null);
  let assignmentLoading = $state(false);
  let assignmentReady = $state(false);
  let assignmentBusy = $state(false);
  let assignmentError = $state('');
  let notes = $state<RepositoryNote[]>([]);
  let notesError = $state('');
  $effect(() => {
    if (!preview && repositoryNotes !== undefined) {
      noteRequest += 1; notes = repositoryNotes; notesError = '';
    }
  });
  let editingNote = $state<string | null>(null);
  let noteName = $state('');
  let noteDescription = $state('');
  let noteBusy = $state(false);
  let noteError = $state('');
  let catalogRequest = 0;
  let noteRequest = 0;
  let assignmentRequest = 0;
  let assignmentTouched = false;
  const cache = new GithubCacheClient();

  function prefetchMembership(name: string, priority = false) {
    if (!preview && !destroyed && connected && !authBusy) cache.prefetch('github_repository_lists', { repository: name }, priority);
  }
  function prefetchStars(items: GithubRepository[]) {
    for (const repository of items) prefetchMembership(repository.fullName);
  }
  function prefetchRepository(name: string) {
    if (preview || destroyed || !connected || authBusy) return;
    cache.prefetch('github_repository', { repository: name }, true);
    cache.prefetch('github_lists', {}, true);
    prefetchMembership(name, true);
  }
  type ViewState = {
    searchText: string; searchQuery: string; page: number; listFilter: string; starPage: number;
    repositories: GithubRepositoryPage | null; activity: GithubActivityPage | null; stars: GithubListRepositoryPage | null;
    selectedName: string | null; detail: GithubRepositoryDetail | null; checked: Set<string>;
    contentError: string; detailError: string; updatedAt: number | null; detailUpdatedAt: number | null; scrollTop: number; detailScrollTop: number; renderedKey: string;
  };
  const views = new Map<string, ViewState>();
  const tabViews = new Map<Tab, string>();
  let resultsRegion = $state<HTMLDivElement>();
  let tabRegion = $state<HTMLDivElement>();
  let updatedAt = $state<number | null>(null);
  let detailUpdatedAt = $state<number | null>(null);
  let listsUpdatedAt = $state<number | null>(null);
  let feedVisited = $state(untrack(() => initialTab === 'feed'));
  let feedScrollTop = 0;
  let feedRegion = $state<HTMLDivElement>();
  let feedConsumer = $state<ReturnType<typeof feedState.acquire>>();
  let websitePreview = $state<import('../lib/types').GithubFeedPage | null>(null);
  const website = $derived(preview ? websitePreview : $feedState.page);
  const websiteError = $derived(preview ? '' : $feedState.transportError || website?.error || '');
  const accountMismatch = $derived(Boolean(status?.account?.login && website?.account && status.account.login.toLowerCase() !== website.account.toLowerCase()));
  const sameAccount = $derived(Boolean(status?.account?.login && website?.account && !accountMismatch));
  let renderedKey = $state('');
  let contentAppend = $state(false);
  const connected = $derived(Boolean(status?.connected && status.account));
  const verified = $derived(connected && status?.verified !== false);
  const hasContent = $derived(tab === 'activity' ? activity !== null : tab === 'starred' ? stars !== null : repositories !== null);
  const showingPrevious = $derived(hasContent && renderedKey !== viewKey());
  const displayPage = $derived((tab === 'activity' ? activity?.page : repositories?.page) ?? page);
  const privateAuthorized = $derived(status?.scopes?.includes('repo') ?? false);
  const listsAuthorized = $derived(status?.scopes?.includes('user') ?? false);

  function viewKey() { return JSON.stringify([tab, tab === 'explore' ? searchQuery : '', tab === 'starred' ? listFilter : '', tab === 'starred' ? 1 : page]); }
  function workspace() { return tab === 'feed' ? feedRegion : resultsRegion; }
  function saveView() {
    if (tab === 'feed') { feedScrollTop = workspace()?.scrollTop ?? 0; return; }
    const key = viewKey();
    views.set(key, { searchText, searchQuery, page, listFilter, starPage, repositories, activity, stars, selectedName, detail,
      checked: new Set(checkedRepositories), contentError, detailError, updatedAt, detailUpdatedAt, scrollTop: resultsRegion?.scrollTop ?? 0, detailScrollTop: detailRegion?.scrollTop ?? 0, renderedKey });
    tabViews.set(tab, key);
  }
  function restoreView(key: string, keepContent = false) {
    const saved = views.get(key);
    listRequest += 1; detailRequest += 1; contentLoading = false; detailLoading = false;
    contentError = ''; detailError = '';
    if (saved) {
      searchText = saved.searchText; searchQuery = saved.searchQuery; page = saved.page; listFilter = saved.listFilter; starPage = saved.starPage;
      repositories = saved.repositories; activity = saved.activity; stars = saved.stars; selectedName = saved.selectedName; detail = saved.detail;
      checkedRepositories = new Set(saved.checked); contentError = saved.contentError; detailError = saved.detailError;
      updatedAt = saved.updatedAt; detailUpdatedAt = saved.detailUpdatedAt; renderedKey = saved.renderedKey;
    } else if (!keepContent) {
      repositories = null; activity = null; stars = null; starPage = 1; updatedAt = null; renderedKey = key;
      checkedRepositories = new Set(); resetDetail();
    } else resetDetail();
    const revision = sessionRevision;
    void tick().then(() => { if (!destroyed && revision === sessionRevision && viewKey() === key) { if (resultsRegion) resultsRegion.scrollTop = saved?.scrollTop ?? 0; if (detailRegion) detailRegion.scrollTop = saved?.detailScrollTop ?? 0; } });
  }
  function captureAnchor() {
    const main = resultsRegion;
    if (!main) return null;
    const top = main.getBoundingClientRect().top;
    const row = [...(resultsRegion?.querySelectorAll<HTMLElement>('[data-github-anchor]') ?? [])].find((item) => item.getBoundingClientRect().bottom > top);
    return { main, scrollTop: main.scrollTop, name: row?.dataset.githubAnchor, offset: row ? row.getBoundingClientRect().top - top : 0 };
  }
  async function restoreAnchor(anchor: ReturnType<typeof captureAnchor>, key: string) {
    await tick();
    if (!anchor || destroyed || !active || viewKey() !== key) return;
    const row = [...(resultsRegion?.querySelectorAll<HTMLElement>('[data-github-anchor]') ?? [])].find((item) => item.dataset.githubAnchor === anchor.name);
    anchor.main.scrollTop = row ? anchor.main.scrollTop + row.getBoundingClientRect().top - anchor.main.getBoundingClientRect().top - anchor.offset : anchor.scrollTop;
  }
  function cacheTime(value: number) { return new Date(value).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
  const visibleItems = $derived(tab === 'starred' ? stars?.items ?? [] : repositories?.items ?? []);
  const filtered = $derived(tab === 'explore' ? visibleItems : filterGithubPage(visibleItems, searchText, notes));
  const hasMore = $derived(tab === 'activity' ? activity?.hasMore ?? false : tab === 'starred' ? stars?.hasMore ?? false : repositories?.hasMore ?? false);
  const waiting = $derived(status?.login?.state === 'waiting');
  const activeList = $derived(lists.find((list) => list.id === listFilter));
  const allVisibleChecked = $derived(filtered.length > 0 && filtered.every((repository) => checkedRepositories.has(repository.fullName)));

  function stopLoginPoll() { if (loginTimer !== undefined) clearTimeout(loginTimer); loginTimer = undefined; }
  function scheduleLoginPoll() {
    stopLoginPoll();
    if (!destroyed && active && !preview && !authBusy && (status?.login?.state === 'waiting' || (connected && status?.verified === false && !status.error && !statusError))) {
      loginTimer = setTimeout(() => { loginTimer = undefined; void refreshStatus(); }, 2_000);
    }
  }
  function clearAccountContent(resetFilters = true) {
    sessionRevision += 1; listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1;
    cache.clear(); views.clear(); tabViews.clear(); updatedAt = null; detailUpdatedAt = null; listsUpdatedAt = null; renderedKey = '';
    repositories = null; activity = null; stars = null; contentError = ''; contentLoading = false;
    lists = []; listsError = ''; listsLoading = false; listFilter = 'all'; starPage = 1;
    checkedRepositories = new Set(); editingList = null; listDelete = null; listBusy = false; listEditError = '';
    assignment = null; assignmentLoading = false; assignmentBusy = false; assignmentReady = false; assignmentError = '';
    editingNote = null; noteBusy = false; noteError = '';
    selectedName = null; detail = null; detailError = ''; detailLoading = false; previousFocus = null;
    editing = null; description = ''; homepage = ''; editConfirmed = false; editError = ''; editBusy = false;
    pendingRepositories = new Set(); addingRepositories = new Set();
    if (resetFilters) { page = 1; searchText = tab === 'explore' ? 'windows' : ''; searchQuery = 'windows'; }
  }
  function acceptStatus(next: GithubStatus) {
    const previousAccount = githubCacheIdentity(status);
    // A failed verification is not logout: retain the cached identity and visible pages.
    if (status?.account && !next.account && next.error && next.verified === false && (next.sessionRevision === undefined || next.sessionRevision === status.sessionRevision)) {
      status = { ...next, connected: status.connected, account: status.account }; return;
    }
    const changedAccount = previousAccount !== githubCacheIdentity(next);
    const changedRevision = status?.sessionRevision !== undefined && next.sessionRevision !== undefined && status.sessionRevision !== next.sessionRevision;
    const linkedRepository = active && linkBack ? selectedName : null;
    status = next;
    const changedCacheSession = cache.setSession(githubCacheIdentity(next), next.sessionRevision);
    // Initial account detection must not erase a search typed while it was loading.
    if (changedAccount) clearAccountContent(previousAccount !== 'anonymous');
    else if (changedRevision) { sessionRevision += 1; listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1; contentLoading = false; detailLoading = false; }
    if (changedAccount || changedRevision || changedCacheSession) { void loadContent(); if (tab === 'starred') void loadLists(); if (linkedRepository) void selectRepository(linkedRepository,false); }
    void preloadContent();
  }
  async function refreshStatus(showLoading = false) {
    if (destroyed || authBusy) return;
    const request = ++statusRequest;
    if (showLoading) statusLoading = true;
    statusError = '';
    try {
      const next = preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewGithubStatus(previewScenario)
        : await dispatch('github_status', {});
      if (!destroyed && request === statusRequest) acceptStatus(next);
    } catch (error) {
      if (!destroyed && request === statusRequest) statusError = readableError(error);
    } finally {
      if (!destroyed && request === statusRequest) { statusLoading = false; scheduleLoginPoll(); }
    }
  }
  async function accountAction(action: 'github_login' | 'github_cancel_login' | 'github_logout', permission?: 'lists' | 'private') {
    if (preview || authBusy || destroyed) return;
    const request = ++statusRequest;
    stopLoginPoll(); authBusy = true; statusError = ''; statusLoading = false;
    if (action === 'github_logout') {
      // Hide private content immediately, not after the logout round-trip.
      clearAccountContent();
      if (status) status = { ...status, connected: false, account: null, login: null };
    }
    try {
      const next = action === 'github_login' ? await dispatch(action, permission === 'lists' ? { lists: true } : permission === 'private' ? { privateRepositories: true } : {}) : await dispatch(action, {});
      // The shared website state must clear even if this page was left while
      // the host finished the explicit logout.
      if (action === 'github_logout') window.dispatchEvent(new Event('releasehub:github-logout'));
      if (!destroyed && request === statusRequest) acceptStatus(next);
    } catch (error) {
      if (!destroyed && request === statusRequest) statusError = readableError(error);
    } finally {
      if (!destroyed && request === statusRequest) {
        authBusy = false; scheduleLoginPoll();
        if (!repositories && !activity && !stars && !contentLoading) void loadContent();
        if (connected && tab === 'starred') void loadLists();
        void preloadContent();
      }
    }
  }
  async function loadContent(append = false, force = false) {
    if (append && contentLoading) return;
    const request = ++listRequest;
    const session = sessionRevision;
    const requestedTab = tab;
    const requestedPage = page;
    const requestedList = listFilter;
    const key = viewKey();
    const previousStars = stars;
    const previousBatches = starPage;
    const cursor = append ? stars?.endCursor ?? undefined : undefined;
    const query = requestedTab === 'explore' ? searchQuery : '';
    contentError = ''; contentLoading = false; contentAppend = append;
    if (destroyed || requestedTab === 'feed' || (requestedTab !== 'explore' && (!connected || authBusy)) || (requestedTab === 'explore' && !query)) return;
    if (append && !cursor) return;
    if (!append && !force && hasContent && renderedKey === key && (
      requestedTab === 'starred' ? cache.isFresh('github_list_repositories', { listId: requestedList }) :
      requestedTab === 'activity' ? cache.isFresh('github_activity', { page: requestedPage }) :
      cache.isFresh('github_repositories', { tab: requestedTab, query, page: requestedPage })
    )) return;
    contentLoading = true;
    const validSession = () => !destroyed && session === sessionRevision;
    const current = () => validSession() && request === listRequest && viewKey() === key;
    const apply = (result: GithubRepositoryPage | GithubActivityPage | GithubListRepositoryPage, time: number, cached = false, batches = append ? previousBatches + 1 : 1) => {
      if (!validSession()) return;
      // A restored view already owns its scroll and cursor chain; only new network data should replace it.
      if (cached && current() && !append && hasContent && renderedKey === key) return;
      const nextStars = requestedTab === 'starred' ? { ...(result as GithubListRepositoryPage), items: append ? appendGithubRepositories(previousStars?.items ?? [], (result as GithubListRepositoryPage).items) : (result as GithubListRepositoryPage).items } : null;
      if (requestedTab === 'starred') prefetchStars((result as GithubListRepositoryPage).items);
      if (!current()) {
        const saved = views.get(key);
        if (saved) views.set(key, { ...saved, ...(requestedTab === 'activity' ? { activity: result as GithubActivityPage } : requestedTab === 'starred' ? { stars: nextStars, starPage: batches } : { repositories: result as GithubRepositoryPage }), updatedAt: time, contentError: '', renderedKey: key });
        return;
      }
      const anchor = renderedKey === key ? captureAnchor() : null;
      if (requestedTab === 'activity') activity = result as GithubActivityPage;
      else if (requestedTab === 'starred') { stars = nextStars; starPage = batches; }
      else repositories = result as GithubRepositoryPage;
      updatedAt = time; renderedKey = key;
      if (!append) checkedRepositories = new Set([...checkedRepositories].filter((name) => (requestedTab === 'starred' ? nextStars?.items : repositories?.items)?.some((item) => item.fullName === name)));
      saveView(); void restoreAnchor(anchor, key);
    };
    try {
      if (requestedTab === 'activity') {
        if (preview && import.meta.env.DEV) apply((await import('../lib/github-preview')).createPreviewGithubActivity(requestedPage, previewScenario), Date.now());
        else { const entry = await cache.load('github_activity', { page: requestedPage }, { force, onCached: (entry) => apply(entry.data, entry.updatedAt, true) }); apply(entry.data, entry.updatedAt); }
      } else if (requestedTab === 'starred') {
        if (preview && import.meta.env.DEV) apply((await import('../lib/github-preview')).createPreviewGithubListRepositories(requestedList, cursor, previewScenario), Date.now());
        else {
          let entry = await cache.load('github_list_repositories', { listId: requestedList, ...(cursor ? { cursor } : {}) }, { force, onCached: (entry) => apply(entry.data, entry.updatedAt, true) });
          // Expired cursor chains are rebuilt offscreen, then replaced together. An explicit refresh starts from the first batch.
          let batches = 1;
          if (!force && !append && previousBatches > 1) {
            while (batches < previousBatches && entry.data.hasMore && entry.data.endCursor && validSession()) {
              const next = await cache.load('github_list_repositories', { listId: requestedList, cursor: entry.data.endCursor }, { force: true });
              entry = { ...next, data: { ...next.data, items: appendGithubRepositories(entry.data.items, next.data.items) } }; batches += 1;
            }
          }
          apply(entry.data, entry.updatedAt, false, append ? previousBatches + 1 : batches);
        }
      } else {
        if (preview && import.meta.env.DEV) apply((await import('../lib/github-preview')).createPreviewGithubRepositories(requestedTab, query, requestedPage, previewScenario), Date.now());
        else { const entry = await cache.load('github_repositories', { tab: requestedTab, query, page: requestedPage }, { force, onCached: (entry) => apply(entry.data, entry.updatedAt, true) }); apply(entry.data, entry.updatedAt); }
      }
    } catch (error) { if (current() && !(error instanceof GithubCacheSuperseded)) { contentError = readableError(error); if (connected) void refreshStatus(); } }
    finally { if (current()) { contentLoading = false; saveView(); } }
  }
  function resetDetail() { detailRequest += 1; selectedName = null; detail = null; detailError = ''; detailLoading = false; detailUpdatedAt = null; }
  function changeTab(next: Tab) {
    if (tab === next) return;
    linkBack = null;
    saveView();
    tab = next; page = 1; searchText = next === 'explore' ? 'windows' : ''; searchQuery = 'windows'; listFilter = 'all';
    restoreView(tabViews.get(next) ?? viewKey());
    if (next === 'feed') { feedVisited = true; void tick().then(() => { if (tab === 'feed' && workspace()) workspace()!.scrollTop = feedScrollTop; }); return; }
    void loadContent();
    if (selectedName) void selectRepository(selectedName, false);
    if (next === 'starred') void loadLists();
  }
  function tabKey(event: KeyboardEvent, index: number) {
    const next = event.key === 'ArrowRight' ? (index + 1) % tabs.length : event.key === 'ArrowLeft' ? (index + tabs.length - 1) % tabs.length : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault(); changeTab(tabs[next].id);
    document.getElementById(`github-tab-${tabs[next].id}`)?.focus();
  }
  function search(event: SubmitEvent) {
    event.preventDefault();
    if (tab !== 'explore') return;
    saveView(); const query = searchText.trim(); searchQuery = query; page = 1;
    restoreView(viewKey()); searchText = query; void loadContent();
  }
  function goPage(next: number) { saveView(); page = Math.max(1, next); restoreView(viewKey(), true); void loadContent(); }
  async function selectRepository(name: string, focus = true, force = false) {
    if (pendingRepositories.has(name)) return;
    if (!force && selectedName === name && detail && cache.isFresh('github_repository', { repository: name })) { if (focus) { await tick(); detailRegion?.focus({ preventScroll: true }); } return; }
    const listAnchor = captureAnchor();
    if (focus) previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (selectedName !== name) { detail = null; detailUpdatedAt = null; if (detailRegion) detailRegion.scrollTop = 0; }
    selectedName = name; detailError = ''; detailLoading = true;
    if (!preview && connected && !authBusy) { cache.prefetch('github_lists', {}, true); prefetchMembership(name, true); }
    const request = ++detailRequest;
    const session = sessionRevision;
    const key = viewKey();
    const current = () => !destroyed && request === detailRequest && session === sessionRevision && selectedName === name && viewKey() === key;
    const apply = (result: GithubRepositoryDetail, time: number) => {
      if (destroyed || session !== sessionRevision) return;
      if (current()) { const anchor = captureAnchor(); detail = result; detailUpdatedAt = time; saveView(); void restoreAnchor(anchor, key); }
      else { const saved = views.get(key); if (saved?.selectedName === name) views.set(key, { ...saved, detail: result, detailUpdatedAt: time, detailError: '' }); }
    };
    await restoreAnchor(listAnchor, key);
    if (focus && current()) detailRegion?.focus({ preventScroll: true });
    try {
      if (preview && import.meta.env.DEV) apply((await import('../lib/github-preview')).createPreviewGithubDetail(name, connected), Date.now());
      else { const entry = await cache.load('github_repository', { repository: name }, { force, onCached: (entry) => apply(entry.data, entry.updatedAt) }); apply(entry.data, entry.updatedAt); }
    } catch (error) { if (current() && !(error instanceof GithubCacheSuperseded)) detailError = readableError(error); }
    finally { if (current()) { detailLoading = false; saveView(); } }
  }
  export async function openRepositoryLink(name: string, returnToSource: () => Promise<void>) {
    const previousTab = tab, previousName = selectedName, previousTop = detailRegion?.scrollTop ?? 0;
    const anchor = captureAnchor(), previousKey = viewKey(), revision = sessionRevision;
    const earlierReturn = linkBack;
    if (tab === 'feed') changeTab('explore');
    linkBack = earlierReturn ?? (async () => {
      if (revision === sessionRevision) {
        if (tab !== previousTab) changeTab(previousTab);
        else if (previousTab !== 'feed') {
          if (previousName) void selectRepository(previousName, false); else resetDetail();
          await restoreAnchor(anchor, previousKey);
          if (detailRegion) detailRegion.scrollTop = previousTop;
        }
      } else resetDetail();
      await returnToSource();
    });
    await selectRepository(name);
  }
  async function closeDetail() {
    if (linkBack) { const back = linkBack; linkBack = null; await back(); return; }
    const anchor = captureAnchor(); const key = viewKey();
    resetDetail(); await restoreAnchor(anchor, key);
    if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  }
  function setRepositoryPending(name: string, value: boolean) {
    const next = new Set(pendingRepositories); if (value) next.add(name); else next.delete(name); pendingRepositories = next;
  }
  function updateVisibleRepository(name: string, update: (repository: GithubRepository) => GithubRepository) {
    cache.invalidate(['github_repositories', 'github_repository', 'github_lists', 'github_list_repositories', 'github_repository_lists']);
    for (const [key, saved] of views) views.set(key, { ...saved,
      repositories: saved.repositories ? { ...saved.repositories, items: saved.repositories.items.map((item) => item.fullName === name ? update(item) : item) } : null,
      stars: saved.stars ? { ...saved.stars, items: saved.stars.items.map((item) => item.fullName === name ? update(item) : item) } : null,
      detail: saved.detail?.repository.fullName === name ? { ...saved.detail, repository: update(saved.detail.repository) } : saved.detail });
    if (repositories) repositories = { ...repositories, items: repositories.items.map((item) => item.fullName === name ? update(item) : item) };
    if (stars) stars = { ...stars, items: stars.items.map((item) => item.fullName === name ? update(item) : item) };
    if (detail?.repository.fullName === name) detail = { ...detail, repository: update(detail.repository) };
  }
  async function setStar(repository: GithubRepository) {
    if (preview || !connected || authBusy || repository.starred === null || pendingRepositories.has(repository.fullName)) return;
    const session = sessionRevision;
    setRepositoryPending(repository.fullName, true);
    try {
      const result = await dispatch('github_set_star', { repository: repository.fullName, starred: !repository.starred, confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      const starDelta = repository.starred === result.starred ? 0 : result.starred ? 1 : -1;
      updateVisibleRepository(repository.fullName, (item) => ({ ...item, starred: result.starred, stars: Math.max(0, item.stars + starDelta) }));
      onnotice(result.starred ? `已 Star ${repository.fullName}。` : `已取消 ${repository.fullName} 的 Star。`);
      if (tab === 'starred') {
        const checked = new Set(checkedRepositories); checked.delete(repository.fullName); checkedRepositories = checked;
        await loadLists(); await loadContent();
      }
    } catch (error) { if (!destroyed && session === sessionRevision) onnotice(readableError(error), 'danger'); }
    finally { if (!destroyed && session === sessionRevision) setRepositoryPending(repository.fullName, false); }
  }
  async function addRepository(repository: GithubRepository) {
    if (preview || repository.private || isSubscribed(repository.fullName, projects) || addingRepositories.has(repository.fullName)) return;
    const session = sessionRevision;
    addingRepositories = new Set([...addingRepositories, repository.fullName]);
    try { await onadd(repository); }
    catch (error) { if (!destroyed && session === sessionRevision) onnotice(readableError(error), 'danger'); }
    finally { if (!destroyed && session === sessionRevision) { const next = new Set(addingRepositories); next.delete(repository.fullName); addingRepositories = next; } }
  }
  function editRepository(repository: GithubRepository) {
    if (!repository.canAdmin || !connected || authBusy || pendingRepositories.has(repository.fullName)) return;
    editing = repository; description = repository.description ?? ''; homepage = repository.homepage ?? ''; editConfirmed = false; editError = '';
  }
  async function saveRepository(event: SubmitEvent) {
    event.preventDefault();
    if (!editing || preview || !connected || authBusy || editBusy || !editing.canAdmin || !editConfirmed) return;
    editError = repositoryEditError(description, homepage) ?? '';
    if (editError) return;
    const name = editing.fullName;
    const session = sessionRevision;
    editBusy = true; setRepositoryPending(name, true);
    try {
      const result = await dispatch('github_update_repository', { repository: name, description: description.trim(), homepage: homepage.trim(), confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      updateVisibleRepository(name, (item) => ({ ...item, description: result.description, homepage: result.homepage, updatedAt: result.updatedAt }));
      editing = null; onnotice('仓库说明与主页已保存到 GitHub。');
    } catch (error) { if (!destroyed && session === sessionRevision) editError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) { editBusy = false; setRepositoryPending(name, false); } }
  }
  async function loadLists(force = false, preferFreshCache = false): Promise<boolean> {
    const request = ++catalogRequest;
    const session = sessionRevision;
    if (!connected || destroyed || authBusy) return false;
    listsLoading = true; listsError = '';
    const current = () => !destroyed && request === catalogRequest && session === sessionRevision;
    const apply = (result: GithubUserList[], time: number) => { if (current()) { lists = result; listsUpdatedAt = time; } };
    try {
      if (preview && import.meta.env.DEV) apply((await import('../lib/github-preview')).createPreviewGithubLists(previewScenario), Date.now());
      else { const entry = await cache.load('github_lists', {}, { force, preferFreshCache, onCached: (entry) => apply(entry.data, entry.updatedAt) }); apply(entry.data, entry.updatedAt); }
      return current();
    } catch (error) { if (current() && !(error instanceof GithubCacheSuperseded)) listsError = readableError(error); }
    finally { if (current()) listsLoading = false; }
    return false;
  }
  let preloadedSession = -1;
  async function preloadContent() {
    if (!preload || preview || destroyed || authBusy || !connected || preloadedSession === sessionRevision) return;
    const session = sessionRevision;
    preloadedSession = session;
    const current = () => !destroyed && session === sessionRevision && connected && !authBusy;
    const jobs: Array<() => Promise<unknown>> = [
      () => cache.load('github_repositories', { tab: 'explore', query: 'windows', page: 1 }),
      () => cache.load('github_repositories', { tab: 'owned', query: '', page: 1 }),
      () => cache.load('github_activity', { page: 1 }),
      async () => { const entry = await cache.load('github_list_repositories', { listId: 'all' }, { preferFreshCache: true }); if (current()) prefetchStars(entry.data.items); },
      () => cache.load('github_list_repositories', { listId: 'unlisted' }),
    ];
    // Warm only a small first batch of categories, never scan hundreds of list pages.
    const catalog = loadLists(false, true);
    const worker = async () => {
      while (current() && jobs.length) {
        const job = jobs.shift()!;
        try { await job(); } catch { /* A failed background refresh must not replace another view or block startup. */ }
      }
    };
    const initial = Promise.all([worker(), worker()]);
    if (await catalog && current()) {
      for (const list of lists.slice(0, 3)) jobs.push(() => cache.load('github_list_repositories', { listId: list.id }, { preferFreshCache: true }));
    }
    await initial;
    await Promise.all([worker(), worker()]);
  }
  function changeList(id: string) {
    if (id === listFilter) return;
    saveView(); listFilter = id; restoreView(viewKey()); void loadContent();
  }
  function editUserList(list?: GithubUserList) {
    if (listBusy) return;
    editingList = list ? { id: list.id, name: list.name, description: list.description ?? '' } : { name: '', description: '' };
    listDelete = null; listEditError = '';
  }
  async function saveUserList(event: SubmitEvent) {
    event.preventDefault();
    if (!editingList || !connected || preview || authBusy || listBusy) return;
    const value = { ...editingList, name: editingList.name.trim(), description: editingList.description.trim() };
    if (!value.name) { listEditError = '请填写分类名称。'; return; }
    const session = sessionRevision;
    listBusy = true; listEditError = '';
    try {
      const saved = await dispatch('github_save_list', { ...value, confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      cache.invalidate(['github_lists', 'github_list_repositories', 'github_repository_lists']); lists = [...lists.filter((list) => list.id !== saved.id), saved]; editingList = null;
      onnotice(`分类“${saved.name}”已保存到 GitHub。`, 'success');
      await loadLists();
    } catch (error) { if (!destroyed && session === sessionRevision) listEditError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) listBusy = false; }
  }
  async function deleteUserList() {
    if (!listDelete || !connected || preview || authBusy || listBusy) return;
    const selected = listDelete;
    const session = sessionRevision;
    listBusy = true; listEditError = '';
    try {
      await dispatch('github_delete_list', { id: selected.id, confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      cache.invalidate(['github_lists', 'github_list_repositories', 'github_repository_lists']); lists = lists.filter((list) => list.id !== selected.id); listDelete = null;
      if (listFilter === selected.id) changeList('all');
      else if (listFilter === 'unlisted') void loadContent(false, true);
      onnotice(`分类“${selected.name}”已删除，Stars 已保留。`, 'success');
      await loadLists();
    } catch (error) { if (!destroyed && session === sessionRevision) listEditError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) listBusy = false; }
  }
  function toggleRepository(name: string) {
    const next = new Set(checkedRepositories);
    if (next.has(name)) next.delete(name);
    else if (next.size < 100) next.add(name);
    else { onnotice('一次最多整理 100 个仓库，请先保存这一批。', 'warning'); return; }
    checkedRepositories = next;
  }
  function toggleVisibleRepositories() {
    const next = new Set(checkedRepositories);
    if (allVisibleChecked) filtered.forEach((repository) => next.delete(repository.fullName));
    else for (const repository of filtered) { if (next.size >= 100) break; next.add(repository.fullName); }
    checkedRepositories = next;
  }
  async function openAssignment(names: string[], mode: 'add' | 'remove' | 'replace') {
    if (!names.length || !connected || authBusy || assignmentBusy) return;
    const request = ++assignmentRequest;
    const session = sessionRevision;
    assignment = { repositories: [...names], mode, listIds: mode === 'remove' && activeList ? [activeList.id] : [] };
    assignmentReady = false; assignmentLoading = true; assignmentError = ''; assignmentTouched = false;
    const current = () => !destroyed && session === sessionRevision && request === assignmentRequest && assignment !== null;
    let catalogRead = false;
    let membership: string[] | null = mode === 'replace' ? null : [];
    const applyMembership = () => {
      if (!current() || !assignment || !catalogRead || membership === null) return;
      if (membership.some((id) => !lists.some((list) => list.id === id))) {
        assignmentReady = false; assignmentError = 'GitHub 分类刚发生了变化，请重新读取归属。'; return;
      }
      // A revalidation may finish after the first user edit; never overwrite those checks.
      if (mode === 'replace' && !assignmentTouched) assignment = { ...assignment, listIds: [...membership] };
      assignmentReady = true; assignmentLoading = false;
    };
    const applyCatalog = (data: GithubUserList[], time: number) => {
      if (!current()) return;
      lists = data; listsUpdatedAt = time; catalogRead = true; applyMembership();
    };
    await tick(); document.getElementById('github-assignment-title')?.focus();
    try {
      if (preview && import.meta.env.DEV) {
        const data = await import('../lib/github-preview');
        membership = mode === 'replace' ? data.createPreviewRepositoryLists(names[0]) : [];
        applyCatalog(data.createPreviewGithubLists(previewScenario), Date.now());
      } else {
        await Promise.all([
          cache.load('github_lists', {}, { preferFreshCache: true, onCached: (entry) => applyCatalog(entry.data, entry.updatedAt) })
            .then((entry) => applyCatalog(entry.data, entry.updatedAt)),
          mode !== 'replace' ? Promise.resolve() : cache.load('github_repository_lists', { repository: names[0] }, {
            preferFreshCache: true, onCached: (entry) => { membership = entry.data.listIds; applyMembership(); },
          }).then((entry) => { membership = entry.data.listIds; applyMembership(); }),
        ]);
      }
    } catch (error) { if (current() && !(error instanceof GithubCacheSuperseded)) assignmentError = readableError(error); }
    finally { if (current()) assignmentLoading = false; }
  }
  function toggleAssignmentList(id: string) {
    if (!assignment || !assignmentReady) return;
    assignmentTouched = true;
    assignment = { ...assignment, listIds: assignment.listIds.includes(id) ? assignment.listIds.filter((entry) => entry !== id) : [...assignment.listIds, id] };
  }
  function closeAssignment() { if (!assignmentBusy) { assignmentRequest += 1; assignment = null; } }
  async function saveAssignment(event: SubmitEvent) {
    event.preventDefault();
    if (!assignment || !assignmentReady || preview || !connected || authBusy || assignmentBusy) return;
    if (!assignment.listIds.length && assignment.mode !== 'replace') { assignmentError = '请至少选择一个分类。'; return; }
    const selected = { ...assignment };
    const session = sessionRevision;
    assignmentBusy = true; assignmentError = '';
    try {
      const result = await dispatch('github_assign_lists', { ...selected, confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      cache.invalidate(['github_lists', 'github_list_repositories', 'github_repository_lists']); assignment = null; checkedRepositories = new Set();
      onnotice(result.updated ? `已同步 ${result.updated} 个仓库的分类。` : '所选仓库的分类已经一致。', 'success');
      await loadLists(); if (tab === 'starred') await loadContent();
    } catch (error) {
      if (!destroyed && session === sessionRevision) { cache.invalidate(['github_lists', 'github_list_repositories', 'github_repository_lists']); assignmentReady = false; assignmentError = readableError(error); void loadLists(); if (tab === 'starred') void loadContent(); }
    } finally { if (!destroyed && session === sessionRevision) assignmentBusy = false; }
  }
  async function loadNotes() {
    if (!preview && repositoryNotes !== undefined) return;
    const request = ++noteRequest;
    notesError = '';
    try {
      const result = preview && import.meta.env.DEV ? (await import('../lib/github-preview')).createPreviewRepositoryNotes() : await dispatch('get_repository_notes', {});
      if (!destroyed && request === noteRequest) notes = result;
    } catch (error) { if (!destroyed && request === noteRequest) notesError = readableError(error); }
  }
  function editNote(repository: string) {
    const note = findRepositoryNote(notes, repository);
    editingNote = repository; noteName = note?.name ?? ''; noteDescription = note?.description ?? ''; noteError = '';
  }
  async function saveNote(event: SubmitEvent) {
    event.preventDefault();
    if (!editingNote || preview || noteBusy) return;
    const repository = editingNote;
    const name = noteName.trim(); const description = noteDescription.trim();
    if ([...name].length > 80 || [...description].length > 240 || /[\u0000-\u001f\u007f]/.test(name + description)) { noteError = '备注名最多 80 字，说明最多 240 字，请使用单行文字。'; return; }
    const session = sessionRevision;
    noteBusy = true; noteError = ''; noteRequest += 1;
    try {
      const result = await dispatch('save_repository_note', { repository, name, description });
      if (destroyed || session !== sessionRevision) return;
      notes = notes.filter((note) => note.repository.toLowerCase() !== repository.toLowerCase());
      if (result) notes = [...notes, result];
      onnotes?.(notes);
      editingNote = null; onnotice(result ? '本地备注已保存。' : '本地备注已清除。', 'success');
      await loadNotes();
    } catch (error) { if (!destroyed && session === sessionRevision) noteError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) noteBusy = false; }
  }
  function changePreview() {
    stopLoginPoll(); statusRequest += 1; clearAccountContent(); status = null;
    void refreshStatus(true); void loadContent(); void loadNotes();
  }
  function clearMemoryCache() {
    cache.clear(); views.clear(); tabViews.clear(); listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1;
    if (assignment) { assignmentReady = false; assignmentLoading = false; assignmentError = '缓存已清除，请重新读取归属。'; }
    contentLoading = false; detailLoading = false; listsLoading = false; updatedAt = null; detailUpdatedAt = null; listsUpdatedAt = null;
    // Settings clears cache, not an in-progress editor or the content currently being read.
  }
  $effect(() => { feedConsumer?.setActive(active && !preview); });
  function focusAccount() {
    document.querySelector<HTMLElement>('.github-account')?.focus({ preventScroll: true });
  }
  $effect(() => {
    if (active) untrack(scheduleLoginPoll);
    else { linkBack = null; stopLoginPoll(); untrack(saveView); }
  });
  onMount(() => {
    if (!preview) { feedConsumer = feedState.acquire(active); if (preload) void feedState.preload(); }
    else if (import.meta.env.DEV) void import('../lib/feed/preview').then((module) => { if (!destroyed) websitePreview = module.previewFeed('connected'); });
    window.addEventListener('releasehub:cache-cleared', clearMemoryCache);
    void refreshStatus(true); void loadContent(); void loadNotes();
    return () => { feedConsumer?.release(); window.removeEventListener('releasehub:cache-cleared', clearMemoryCache); cache.clear(); destroyed = true; statusRequest += 1; listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1; noteRequest += 1; sessionRevision += 1; stopLoginPoll(); };
  });
</script>

<div class="page-heading"><h1>GitHub</h1></div>

{#if preview}
  <div class="github-preview"><span><strong>GitHub 界面示例</strong> · 演示数据，不访问真实账号</span><label>示例状态<select aria-label="GitHub 示例状态" bind:value={previewScenario} onchange={changePreview}><option value="connected">已连接</option><option value="anonymous">未登录</option><option value="waiting">等待短码授权</option><option value="unavailable">缺少登录组件</option><option value="empty">空列表</option><option value="error">读取失败</option></select></label></div>
{/if}

<section class="surface github-account" class:awaiting={waiting} aria-label="GitHub 账号" tabindex="-1">
  <div class="account-overview">
  <div class="account-line">
    <GithubAvatar url={status?.account?.avatarUrl} name={status?.account?.login ?? 'GitHub'} />
    <div class="account-identity" aria-live="polite">
      {#if statusLoading}<strong>正在读取账号状态…</strong>
      {:else if waiting}<strong>等待 GitHub 授权</strong><p>完成后本页自动更新</p>
      {:else if connected && status?.account}<strong>{status.account.name || status.account.login}{#if preview}<span class="status-badge warning">示例账号</span>{/if}</strong><p>@{status.account.login} · {status?.verified === false ? '本机已恢复，待验证' : '独立授权已连接'}</p>
      {:else if status?.available === false}<strong>没有找到 GitHub 登录组件</strong><p>公开仓库仍可查看</p>
      {:else}<strong>GitHub 账号</strong><p>公开仓库无需登录</p>{/if}
    </div>
    <div class="account-actions"><button class="icon-button" aria-label="刷新 GitHub 账号状态" disabled={authBusy || statusLoading} onclick={() => void refreshStatus(true)}><Icon name="refresh" size={16} /></button>{#if waiting}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_cancel_login')}>{authBusy ? '正在取消' : '取消登录'}</button>{:else if connected}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_logout')}><Icon name="logout" size={15} />仅断开追新</button>{:else}<button class="button primary" disabled={authBusy || statusLoading || status?.available === false || preview} onclick={() => void accountAction('github_login')}><Icon name={authBusy ? 'loader' : 'github'} class={authBusy ? 'spinning' : ''} size={16} />{authBusy ? '正在连接' : '登录 GitHub'}</button>{/if}</div>
  </div>
  <div class="website-account">
    <div><strong>官网首页动态{#if accountMismatch}<span class="status-badge warning">账号不同</span>{/if}</strong><p>{#if !website && !websiteError}正在读取官网会话…{:else if website?.account}@{website.account} · {website.cached ? '历史账号，待验证' : website.connected ? '官网已登录' : '会话待验证'}{:else}官网未登录{/if}</p></div>
    <button class="button secondary small" disabled={preview || Boolean($feedState.pending)} onclick={() => void feedState.action('feed_connect')}><Icon name="github" size={14} />{website?.connected ? '查看官网账号' : '官网登录'}</button>
    {#if website?.account}<button class="icon-button" aria-label="验证官网会话" disabled={preview || Boolean($feedState.pending) || website?.loading} onclick={() => void feedState.action('feed_refresh')}><Icon name="refresh" size={15} /></button>{/if}
  </div>
  </div>
  {#if websiteError}<p class="field-error website-error" role="alert">官网会话：{websiteError}</p>{/if}
  {#if waiting}<div class="device-login"><div><label for="github-device-code">一次性登录短码</label><div class="code-line"><input id="github-device-code" class="mono" readonly value={status?.login?.userCode ?? '正在获取短码…'} /><button class="button secondary" disabled={!status?.login?.userCode} onclick={() => oncopy(status!.login!.userCode!)}><Icon name="copy" size={15} />复制短码</button></div></div><div><button class="button primary" disabled={!status?.login?.userCode || preview} onclick={() => onopen(GITHUB_DEVICE_URL)}>打开 GitHub 授权页<Icon name="external" size={15} /></button><p>在 github.com/login/device 核对账号并输入短码</p></div></div>{/if}
  {#if statusError || status?.error || status?.login?.state === 'error'}<p class="field-error" role="alert">{statusError || status?.error || status?.login?.message || '登录未完成，请重新登录。'}</p>{/if}
  <details class="account-help"><summary>授权、官网会话与权限说明</summary>
    {#if accountMismatch}<p class="account-mismatch" role="status">{website?.cached ? '历史官网账号' : '官网账号'} @{website?.account} 与仓库授权 @{status?.account?.login} 不同。动态来自官网，仓库和 Stars 来自授权账号；请在官网窗口核对后刷新。</p>{/if}
    {#if sameAccount}<p class="account-consistency" role="status">{website?.cached || !website?.connected ? '已保存的账号一致，官网会话仍待验证。' : '仓库授权与官网登录账号一致。'}</p>{/if}
    <p class="account-note">ReleaseHub 独立授权，不使用或断开其它工具的 GitHub 账号。基本登录申请读取账号（read:user）、公开仓库读写（public_repo）和保持登录（offline_access）；凭据只保存在本机凭据库，不随便携数据迁移。旧版连接需要重新授权一次。首页动态另用 GitHub 网页登录，不随此授权自动连接。</p>
    <p class="website-note">仓库授权不会生成官网 Cookie。官网只需首次明确登录一次，网页会话持久保存在这台电脑；代理切换不会清除它，后台失效只提示，不弹窗。“仅断开追新”会清除追新的授权、网页登录与动态，不影响外部浏览器。</p>
    {#if connected && !waiting}<div class="account-permissions"><button class="text-button" disabled={authBusy || preview} onclick={() => void accountAction('github_login')}>{verified ? '重新授权 ReleaseHub' : '重新连接并验证'}</button>{#if privateAuthorized}<span>已授权私有仓库访问</span>{:else}<details><summary>需要查看私有仓库？</summary><p>GitHub 的 repo 权限允许读写公开与私有仓库，范围比“只读”更大。只有你主动点击下面按钮后才会申请；ReleaseHub 不会自动提交代码。</p><button class="button secondary small" disabled={authBusy || preview} onclick={() => void accountAction('github_login', 'private')}>授权私有仓库（repo）</button></details>{/if}</div>{/if}
  </details>
</section>

<div class="github-tabs" bind:this={tabRegion} role="tablist" aria-label="GitHub 内容">{#each tabs as item, index}<button id={`github-tab-${item.id}`} role="tab" aria-selected={tab === item.id} aria-controls="github-results" tabindex={tab === item.id ? 0 : -1} class:active={tab === item.id} onclick={() => changeTab(item.id)} onkeydown={(event) => tabKey(event, index)}><Icon name={item.icon} size={16} />{item.label}</button>{/each}</div>

{#if feedVisited}
  <div class="github-feed-region" bind:this={feedRegion} id={tab === 'feed' ? 'github-results' : 'github-feed-results'} role="tabpanel" aria-labelledby="github-tab-feed" tabindex="0" hidden={tab !== 'feed'}><GithubFeed {preview} active={active && tab === 'feed'} {onopen} {onadd} {onnotice} onLogin={focusAccount} /></div>
{/if}
{#if tab !== 'feed'}
<div class="github-repository-workspace">
<div class="github-controls">
  {#if tab === 'starred' && connected}
    <section class="lists-toolbar" aria-label="GitHub 分类">
      <div class="lists-topline"><strong><Icon name="folder" size={16} />GitHub 分类</strong><span>{listsLoading ? '正在更新分类…' : `${lists.length} 个分类`}{#if listsUpdatedAt} · 更新于 {cacheTime(listsUpdatedAt)}{/if}</span><button class="text-button" disabled={listsLoading} onclick={() => void loadLists(true)}><Icon name="refresh" size={14} />刷新分类</button><button class="button secondary small" disabled={authBusy || listBusy || preview} onclick={() => editUserList()}><Icon name="plus" size={14} />新建分类</button></div>
      <div class="list-options" aria-label="选择 Stars 分类"><button class:active={listFilter === 'all'} aria-pressed={listFilter === 'all'} onclick={() => changeList('all')}>全部 Stars</button><button class:active={listFilter === 'unlisted'} aria-pressed={listFilter === 'unlisted'} onclick={() => changeList('unlisted')}>未分类</button>{#each lists as list (list.id)}<button class:active={listFilter === list.id} aria-pressed={listFilter === list.id} title={list.description ?? list.name} onclick={() => changeList(list.id)}>{list.name}<span>{list.itemCount}</span></button>{/each}</div>
      {#if activeList}<div class="active-list-info"><p>{activeList.description || '这个分类还没有说明。'}</p><div><button class="text-button" disabled={listBusy || preview} onclick={() => editUserList(activeList)}><Icon name="edit" size={13} />改名与说明</button><button class="text-button" onclick={() => onopen(activeList.url)}>官网分类<Icon name="external" size={13} /></button><button class="text-button danger-link" disabled={listBusy || preview} onclick={() => { listDelete = activeList ?? null; listEditError = ''; }}><Icon name="trash" size={13} />删除分类</button></div></div>{/if}
      {#if !lists.length && !listsLoading && !listsError}<p class="field-hint">新建第一个分类，开始整理 Stars。同一个仓库可放入多个分类。</p>{/if}
      {#if listsError}<p class="field-error" role="alert">{listsError}</p>{/if}
      <div class="list-authorization"><span>分类与 GitHub Lists 同步。分类管理需 user 权限：GitHub 同时允许修改个人资料，并非仅管理分类；只有你主动授权才会申请。</span><button class="text-button" disabled={preview || authBusy || waiting || assignmentBusy || listBusy} onclick={() => void accountAction('github_login', 'lists')}>{listsAuthorized ? '重新授权分类管理' : '授权分类管理'}<Icon name="external" size={13} /></button></div>
      {#if editingList}<form class="list-editor" onsubmit={saveUserList}><h3>{editingList.id ? '改名与说明' : '新建 GitHub 分类'}</h3><div class="list-editor-fields"><label>分类名称<input bind:value={editingList.name} maxlength="100" required disabled={listBusy} placeholder="例如：桌面工具" /></label><label>分类说明 <span class="optional">可选</span><input bind:value={editingList.description} maxlength="1000" disabled={listBusy} placeholder="这个分类收集哪些仓库" /></label></div>{#if listEditError}<p class="field-error" role="alert">{listEditError}</p>{/if}<div class="inline-actions"><button class="button secondary small" type="button" disabled={listBusy} onclick={() => editingList = null}>取消</button><button class="button primary small" type="submit" disabled={listBusy || preview || !editingList.name.trim()}>{listBusy ? '正在保存并回读' : editingList.id ? '保存分类到 GitHub' : '创建分类到 GitHub'}</button></div></form>{/if}
    </section>
  {/if}
  {#if tab !== 'activity'}
    <form class="github-search" onsubmit={search}><div class="search-line"><div class="input-leading"><Icon name={tab === 'explore' ? 'search' : 'filter'} size={17} /><input id="github-search" aria-label={tab === 'explore' ? '搜索 GitHub 仓库' : tab === 'starred' ? '筛选已加载 Stars' : '筛选本页'} bind:value={searchText} maxlength="256" placeholder={tab === 'explore' ? '搜索 GitHub 仓库（支持 topic: 等语法）' : '筛选仓库名、备注或语言'} disabled={tab !== 'explore' && !connected} />{#if searchText}<button type="button" class="icon-button" aria-label="清空仓库搜索" onclick={() => searchText = ''}><Icon name="close" size={14} /></button>{/if}</div>{#if tab === 'explore'}<button class="button primary" type="submit">搜索</button>{/if}</div></form>
  {/if}
  {#if notesError}<div class="notes-error"><p class="field-error">本地备注读取失败：{notesError}</p><button class="text-button" onclick={() => void loadNotes()}>重试备注</button></div>{/if}
  {#if assignment && tab === 'starred'}<section class="assignment-editor surface" aria-label="整理 GitHub 分类"><form onsubmit={saveAssignment}><div class="assignment-heading"><h2 id="github-assignment-title" tabindex="-1">{assignment.mode === 'replace' ? '设置仓库分类' : assignment.mode === 'add' ? '批量添加到分类' : '批量从分类移出'}</h2><button class="icon-button" type="button" aria-label="关闭分类整理" disabled={assignmentBusy} onclick={closeAssignment}><Icon name="close" size={16} /></button></div><p>{assignment.repositories.length === 1 ? assignment.repositories[0] : `已选择 ${assignment.repositories.length} 个仓库`}</p>{#if assignmentLoading}<p role="status">正在读取现有分类归属…</p>{:else}<div class="assignment-options">{#each lists as list (list.id)}<label><input type="checkbox" checked={assignment.listIds.includes(list.id)} disabled={assignmentBusy || !assignmentReady} onchange={() => toggleAssignmentList(list.id)} />{list.name}<span>{list.itemCount}</span></label>{:else}<p>还没有可用分类，请先新建分类。</p>{/each}</div>{/if}{#if assignmentError}<p class="field-error" role="alert">{assignmentError}</p>{/if}{#if !assignmentReady && !assignmentLoading}<button class="text-button" type="button" onclick={() => { if (assignment) void openAssignment(assignment.repositories, assignment.mode); }}>重新读取归属</button>{/if}<div class="inline-actions"><button class="button secondary small" type="button" disabled={assignmentBusy} onclick={closeAssignment}>取消</button><button class="button primary small" type="submit" disabled={preview || assignmentBusy || !assignmentReady || (assignment.mode !== 'replace' && !assignment.listIds.length)}>{assignmentBusy ? '正在同步并回读' : '确认同步分类'}</button></div></form></section>{/if}

</div>
<div class="github-columns" class:with-detail={selectedName !== null}>
  <div class="surface github-results" bind:this={resultsRegion} class:compact-hidden={selectedName !== null} id="github-results" role="tabpanel" tabindex="0" aria-labelledby={`github-tab-${tab}`} aria-busy={contentLoading}>
    {#if hasContent && (contentLoading || showingPrevious)}<div class="github-cache-status" role="status"><span>{contentLoading ? (showingPrevious ? `正在读取第 ${page} 页，保留上一页内容` : '正在后台更新…') : '新页面未读到，显示上一页内容'}</span></div>{/if}
    {#if hasContent && contentError}<div class="page-load-error"><p class="field-error" role="alert">更新未完成，已保留原内容：{contentError}</p><button class="text-button" onclick={() => void loadContent(contentAppend, true)}>{contentAppend ? '重试下一批' : '重试这一页'}</button></div>{/if}
    {#if tab !== 'explore' && !connected}
      <div class="empty-state spacious"><Icon name={tab === 'starred' ? 'star' : tab === 'owned' ? 'user' : 'activity'} size={29} /><h3>登录后查看{tab === 'starred' ? '你的 Stars' : tab === 'owned' ? '自己的仓库' : 'GitHub 动态'}</h3><button class="button secondary" onclick={() => changeTab('explore')}>浏览公开仓库</button></div>
    {:else if contentLoading && !hasContent}
      <div class="repo-loading" role="status"><p>正在读取{tab === 'activity' ? '近期活动' : '仓库列表'}…</p>{#each [1, 2, 3] as row}<div class="loading-row" aria-hidden="true"><span></span><div><i></i><i></i></div></div>{/each}</div>
    {:else if contentError && !hasContent}
      <div class="empty-state spacious"><Icon name="alert" size={27} /><h3>暂时没有读到{tab === 'activity' ? '活动' : '仓库'}</h3><p class="break-word" role="alert">{contentError}</p><button class="button secondary" onclick={() => void loadContent(false, true)}>重试这一页</button></div>
    {:else if tab === 'explore' && !searchQuery}
      <div class="empty-state spacious"><Icon name="search" size={29} /><h3>搜索公开仓库</h3></div>
    {:else if tab === 'activity'}
      <div class="results-heading"><h2>近期活动</h2><span>第 {displayPage} 页{#if updatedAt} · {cacheTime(updatedAt)}{/if}</span><button class="icon-button" aria-label="刷新 GitHub 活动" disabled={contentLoading} onclick={() => void loadContent(false, true)}><Icon name="refresh" size={15} /></button></div>
      {#each activity?.items ?? [] as item (item.id)}<article class="activity-row" data-github-anchor={item.id}><GithubAvatar url={item.avatarUrl} name={item.actor} /><div><p><strong>{item.actor}</strong> {item.summary}</p><button class="text-button repo-link" onclick={() => void selectRepository(item.repository)}>{item.repository}<Icon name="chevronRight" size={13} /></button><span>{formatDate(item.createdAt, true)}</span></div><button class="icon-button" aria-label={`在 GitHub 查看 ${item.actor} 的活动`} onclick={() => onopen(item.url)}><Icon name="external" size={15} /></button></article>{:else}<div class="empty-state"><Icon name="activity" size={27} /><h3>暂无近期活动</h3></div>{/each}
    {:else}
      <div class="results-heading"><h2>{tab === 'explore' ? '搜索结果' : tab === 'owned' ? '我的仓库' : activeList?.name ?? (listFilter === 'unlisted' ? '未分类 Stars' : '我的 Stars')}</h2><span>{tab === 'explore' && repositories?.totalCount !== null && repositories?.totalCount !== undefined ? `约 ${repositories.totalCount.toLocaleString()} 个结果` : tab === 'starred' ? `显示 ${filtered.length} · 已加载 ${stars?.items.length ?? 0} / ${stars?.totalCount ?? 0}` : `本页 ${filtered.length} / ${repositories?.items.length ?? 0} 个`}{#if updatedAt} · {cacheTime(updatedAt)}{/if}</span><button class="icon-button" aria-label="刷新仓库列表" disabled={contentLoading || assignmentBusy} onclick={() => { void loadContent(false, true); if (tab === 'starred') void loadLists(true); void loadNotes(); }}><Icon name="refresh" size={15} /></button></div>
      {#if tab === 'starred' && visibleItems.length}<div class="batch-toolbar"><label><input type="checkbox" aria-label="选择筛选结果" checked={allVisibleChecked} disabled={assignmentBusy} onchange={toggleVisibleRepositories} />选择当前结果</label><span>已选 {checkedRepositories.size} / 100</span><button class="button secondary small" disabled={!checkedRepositories.size || assignmentBusy || preview} onclick={() => void openAssignment([...checkedRepositories], 'add')}>添加到分类</button><button class="button secondary small" disabled={!checkedRepositories.size || assignmentBusy || preview} onclick={() => void openAssignment([...checkedRepositories], 'remove')}>从分类移出</button>{#if checkedRepositories.size}<button class="text-button" disabled={assignmentBusy} onclick={() => checkedRepositories = new Set()}>清空选择</button>{/if}</div>{/if}
      {#each filtered as repository (repository.id)}
        {@const note = findRepositoryNote(notes, repository.fullName)}
        <article class="repo-row" data-github-anchor={repository.fullName} class:selected={selectedName === repository.fullName}>
          <div class="repo-select-line">{#if tab === 'starred'}<input type="checkbox" class="repo-checkbox" aria-label={`选择 ${repository.fullName}`} checked={checkedRepositories.has(repository.fullName)} disabled={assignmentBusy || pendingRepositories.has(repository.fullName)} onchange={() => toggleRepository(repository.fullName)} />{/if}<button class="repo-main" aria-label={`查看 ${repository.fullName} 详情`} aria-pressed={selectedName === repository.fullName} onmouseenter={() => prefetchRepository(repository.fullName)} onfocus={() => prefetchRepository(repository.fullName)} onclick={() => void selectRepository(repository.fullName)}><span class="repo-title"><Icon name={repository.private ? 'private' : 'readme'} size={16} /><strong>{note?.name || repository.fullName}</strong>{#if repository.private}<span class="status-badge neutral">私有</span>{/if}{#if repository.archived}<span class="status-badge warning">已归档</span>{/if}</span>{#if note?.description}<span class="repo-summary local-note-summary">{note.description}</span>{/if}{#if note?.name}<span class="original-name">{repository.fullName}</span>{/if}{#if repository.description}<span class="repo-summary upstream-summary">{repository.description}</span>{/if}</button></div>
          <div class="repo-row-bottom"><div class="repo-meta"><span><Icon name="star" size={13} />{repository.stars.toLocaleString()}{#if repository.starred === true}<span class="starred-label">已 Star</span>{/if}</span>{#if repository.language}<span>{repository.language}</span>{/if}<span>{formatDate(repository.updatedAt)}</span></div><div class="github-row-actions"><button class="icon-button" aria-label={`编辑 ${repository.fullName} 本地备注`} disabled={preview} onclick={() => editNote(repository.fullName)}><Icon name="notes" size={15} /></button>{#if tab === 'starred'}<button class="icon-button" aria-label={`设置 ${repository.fullName} 分类`} disabled={preview || assignmentBusy} onmouseenter={() => prefetchRepository(repository.fullName)} onfocus={() => prefetchRepository(repository.fullName)} onclick={() => void openAssignment([repository.fullName], 'replace')}><Icon name="folder" size={15} /></button>{/if}<button class="button secondary small" disabled={preview || repository.private || isSubscribed(repository.fullName, projects) || addingRepositories.has(repository.fullName)} title={repository.private ? '私有仓库暂不加入资源库' : '加入资源库'} onclick={() => void addRepository(repository)}><Icon name={addingRepositories.has(repository.fullName) ? 'loader' : isSubscribed(repository.fullName, projects) ? 'check' : 'plus'} class={addingRepositories.has(repository.fullName) ? 'spinning' : ''} size={14} />{isSubscribed(repository.fullName, projects) ? '已订阅' : addingRepositories.has(repository.fullName) ? '正在加入' : '加入资源库'}</button></div></div>
        </article>
      {:else}<div class="empty-state"><Icon name={tab === 'starred' ? 'star' : 'search'} size={27} /><h3>{tab === 'starred' ? searchText.trim() ? '已加载的 Stars 中没有匹配结果' : hasMore ? '这批 Stars 没有符合分类的仓库' : '这个分类还没有仓库' : tab === 'owned' && searchText.trim() ? '本页没有匹配的仓库' : '这一页没有找到仓库'}</h3>{#if tab !== 'explore' && searchText.trim()}<button class="button secondary" onclick={() => searchText = ''}>{tab === 'starred' ? '清空 Stars 筛选' : '清空本页筛选'}</button>{/if}</div>{/each}
    {/if}
    {#if tab === 'starred' && stars}<div class="github-pagination stars-pagination"><span>已加载 {starPage} 批 · {stars.items.length} / {stars.totalCount} 个{!hasMore ? ' · 已全部加载' : ''}</span><button class="button secondary small" disabled={!hasMore || contentLoading || assignmentBusy} onclick={() => void loadContent(true)}><Icon name={contentLoading ? 'loader' : 'plus'} class={contentLoading ? 'spinning' : ''} size={14} />{contentLoading ? '正在加载' : hasMore ? '继续加载 Stars' : '已全部加载'}</button></div>
    {:else if (tab === 'explore' || connected) && (repositories || activity)}<div class="github-pagination"><span>第 {displayPage} 页</span><div><button class="button secondary small" disabled={contentLoading || page <= 1} onclick={() => goPage(page - 1)}>上一页</button><button class="button secondary small" disabled={contentLoading || !hasMore || showingPrevious} onclick={() => goPage(page + 1)}>下一页</button></div></div>{/if}
  </div>
  {#if selectedName}<div class="detail-region" bind:this={detailRegion} tabindex="-1">{#if detail && (detailLoading || detailError)}<div class="github-cache-status" role="status">{#if detailLoading}<span>正在更新详情…</span>{/if}{#if detailError}<p class="field-error">更新未完成，已保留原详情：{detailError}</p>{/if}</div>{/if}<GithubDetail name={selectedName} {detail} note={findRepositoryNote(notes, selectedName)} loading={detailLoading && !detail} error={detail ? '' : detailError} connected={verified} {preview} pending={pendingRepositories.has(selectedName)} subscribed={isSubscribed(selectedName, projects)} adding={addingRepositories.has(selectedName)} backLabel={linkBack ? '返回' : '返回列表'} onclose={() => void closeDetail()} onretry={() => { if (selectedName) void selectRepository(selectedName, false, true); }} onstar={(repository) => void setStar(repository)} onedit={editRepository} onnote={editNote} onlists={(repository) => void openAssignment([repository.fullName], 'replace')} onadd={(repository) => void addRepository(repository)} {onopen} {onoriginal} /></div>{/if}
</div>
</div>
{/if}

{#if listDelete}
  <Modal title="删除 GitHub 分类" description={`删除“${listDelete.name}”及其分类归属，仓库的 Star 和其它分类会保留。`} onclose={() => { if (!listBusy) listDelete = null; }}><div class="modal-form">{#if listEditError}<p class="field-error" role="alert">{listEditError}</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={listBusy} onclick={() => listDelete = null}>保留分类</button><button class="button danger" disabled={listBusy || preview} onclick={() => void deleteUserList()}>{listBusy ? '正在删除并回读' : '确认删除分类'}</button></div></div></Modal>
{/if}

{#if editingNote}
  <Modal title="本地备注" description={editingNote} onclose={() => { if (!noteBusy) editingNote = null; }}><form class="modal-form" onsubmit={saveNote}><label>备注名<input bind:value={noteName} maxlength="80" disabled={noteBusy} data-initial-focus placeholder="留空显示原仓库名" /></label><label>备注说明<input bind:value={noteDescription} maxlength="240" disabled={noteBusy} placeholder="用途或线索" /></label>{#if noteError}<p class="field-error" role="alert">{noteError}</p>{/if}<div class="modal-footer"><button class="button secondary" type="button" disabled={noteBusy} onclick={() => editingNote = null}>取消</button><button class="button primary" type="submit" disabled={noteBusy || preview}>{noteBusy ? '正在保存' : '保存本地备注'}</button></div></form></Modal>
{/if}

{#if editing}
  <Modal title="修改仓库说明与主页" description={editing.fullName} onclose={() => { if (!editBusy) editing = null; }}>
    <form class="modal-form" onsubmit={saveRepository}><label>仓库说明<textarea bind:value={description} maxlength="350" rows="3" disabled={editBusy} data-initial-focus placeholder="仓库简介，留空可移除"></textarea></label><label>主页地址<input type="url" bind:value={homepage} maxlength="2000" disabled={editBusy} placeholder="https://example.com" /></label><label class="confirmation-check"><input type="checkbox" bind:checked={editConfirmed} disabled={editBusy || preview} required />我确认将以上说明与主页保存到 GitHub。</label>{#if preview}<p class="field-hint">示例模式，无法提交到真实仓库。</p>{/if}{#if editError}<p class="field-error" role="alert">{editError}</p>{/if}<div class="modal-footer"><button class="button secondary" type="button" disabled={editBusy} onclick={() => editing = null}>取消</button><button class="button primary" type="submit" disabled={editBusy || !editConfirmed || preview}><Icon name={editBusy ? 'loader' : 'check'} class={editBusy ? 'spinning' : ''} size={16} />{editBusy ? '正在保存' : '确认保存到 GitHub'}</button></div></form>
  </Modal>
{/if}

<style>
  .github-preview { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px 20px; color: var(--warning); background: var(--warning-bg); border: 1px solid var(--border); border-radius: 12px; margin-bottom: 14px; padding: 8px 12px; font-size: 12px; }
  .github-preview label { display: flex; gap: 8px; align-items: center; }
  .github-preview select { padding: 4px 8px; font-size: 12px; }
  .page-heading { flex: 0 0 auto; margin-bottom: 14px; }
  .github-preview { flex: 0 0 auto; }
  .github-account { padding: 10px 14px; flex: 0 1 auto; min-height: 0; max-height: 160px; overflow-y: auto; scrollbar-gutter: stable; overscroll-behavior: contain; }
  .github-account.awaiting { max-height: 190px; }
  .github-account:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
  .account-overview { display: grid; grid-template-columns: minmax(0, 1.12fr) minmax(0, 1fr); gap: 10px 18px; }
  .website-account { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 9px; border-left: 1px solid var(--border); padding-left: 16px; }
  .website-account > div { flex: 1; min-width: 120px; }
  .account-help { margin-top: 6px; font-size: 12px; }
  .account-help > summary { width: fit-content; cursor: pointer; color: var(--accent); }
  .account-help > summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; }
  .website-account strong { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 13px; color: var(--heading); }
  .website-account p, .website-note, .account-consistency { font-size: 12px; color: var(--muted); line-height: 1.7; }
  .website-note { margin-top: 7px; max-width: 75ch; }
  .account-mismatch { margin-top: 9px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 5px; font-size: 12px; line-height: 1.7; background: var(--warning-bg); color: var(--warning); overflow-wrap: anywhere; }
  .account-consistency { margin-top: 6px; }
  .website-error { margin-top: 7px; }
  .github-repository-workspace { display: grid; grid-template-columns: minmax(180px, 240px) minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); gap: 16px; flex: 1 1 0; min-height: 0; margin-top: 16px; overflow: hidden; }
  .github-controls { min-width: 0; min-height: 0; overflow-y: auto; scrollbar-gutter: stable; overscroll-behavior: contain; padding-right: 8px; }
  .github-feed-region { flex: 1; min-height: 0; overflow-y: auto; margin-top: 16px; scrollbar-gutter: stable; overscroll-behavior: contain; }
  .github-feed-region[hidden] { display: none; }
  .account-line { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 9px; }
  .account-identity { flex: 1; min-width: 140px; }
  .account-identity strong { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; color: var(--heading); font-size: 13px; overflow-wrap: anywhere; }
  .account-identity p { color: var(--muted); font-size: 13px; margin-top: 2px; }
  .account-actions { display: flex; gap: 6px; align-items: center; flex-shrink: 0; margin-left: auto; }
  .account-note { color: var(--muted); font-size: 13px; margin-top: 13px; padding-top: 11px; border-top: 1px solid var(--border); line-height: 1.8; }
  .device-login { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 14px 22px; border-top: 1px solid var(--border); padding-top: 14px; margin-top: 14px; }
  .device-login label { display: block; font-size: 12px; color: var(--subtle); margin-bottom: 5px; }
  .code-line { display: flex; gap: 8px; align-items: center; }
  .code-line input { width: 178px; padding: 7px 10px; font-size: 17px; font-weight: 650; letter-spacing: .04em; }
  .device-login p { color: var(--muted); font-size: 12px; margin-top: 6px; }
  .github-tabs { display: flex; flex: 0 0 auto; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 10px; padding: 5px; width: fit-content; max-width: 100%; background: var(--surface-soft); border-radius: 24px; }
  .github-tabs button { display: flex; align-items: center; gap: 7px; color: var(--subtle); font-size: 13px; padding: 9px 14px; border-radius: 20px; }
  .github-tabs button:hover { color: var(--accent); background: var(--accent-soft); }
  .github-tabs button.active { color: var(--accent); background: var(--surface); font-weight: 650; }
  .github-tabs button:focus-visible, .account-permissions summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; }
  .github-cache-status { padding: 10px 16px; color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .account-permissions { display: flex; align-items: flex-start; flex-wrap: wrap; gap: 12px 20px; color: var(--subtle); font-size: 13px; margin-top: 12px; }
  .account-permissions details { flex: 1; min-width: 200px; }
  .account-permissions summary { cursor: pointer; color: var(--accent); width: fit-content; }
  .account-permissions p { max-width: 72ch; margin: 10px 0; line-height: 1.7; }
  .github-search { margin: 10px 0; }
  .github-controls .search-line { flex-wrap: wrap; }
  .github-controls .search-line > .input-leading { width: 100%; flex-basis: 100%; min-width: 0; }
  .github-controls .search-line > .button { margin-left: auto; }
  .github-controls .list-options > button { justify-content: space-between; text-align: left; }
  .search-line { display: flex; gap: 9px; }
  .search-line .input-leading { flex: 1; background: var(--surface); }
  .search-line input { font-size: 13px; }
  .search-line .icon-button { margin-right: 4px; }
  .github-columns { display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); gap: 14px; min-width: 0; min-height: 0; align-items: stretch; }
  .github-columns.with-detail { grid-template-columns: minmax(0, .9fr) minmax(0, 1.1fr); }
  .github-results, .detail-region { min-width: 0; min-height: 0; overflow-y: auto; scrollbar-gutter: stable; overscroll-behavior: contain; }
  .github-results { border-radius: 8px; }
  .github-controls::-webkit-scrollbar, .github-results::-webkit-scrollbar, .detail-region::-webkit-scrollbar, .github-feed-region::-webkit-scrollbar, .github-account::-webkit-scrollbar { width: 8px; }
  .github-controls::-webkit-scrollbar-thumb, .github-results::-webkit-scrollbar-thumb, .detail-region::-webkit-scrollbar-thumb, .github-feed-region::-webkit-scrollbar-thumb, .github-account::-webkit-scrollbar-thumb { background: var(--border); border-radius: 5px; }
  .detail-region:focus { outline: none; }
  .detail-region:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; border-radius: 8px; }
  .results-heading { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: var(--surface-soft); }
  .results-heading h2 { font-size: 12px; }
  .results-heading > span { margin-left: auto; color: var(--muted); font-size: 12px; }
  .repo-row { padding: 14px 16px; border-top: 1px solid var(--border); }
  .repo-row:hover { background: var(--surface-soft); }
  .repo-row.selected { background: var(--accent-soft); }
  .repo-main { display: block; width: 100%; min-width: 0; padding: 0; text-align: left; }
  .repo-title { display: flex; align-items: center; gap: 7px; color: var(--accent); flex-wrap: wrap; }
  .repo-title strong { font-size: 15px; font-weight: 650; overflow-wrap: anywhere; }
  .repo-summary.local-note-summary { padding: 7px 10px; background: var(--surface-soft); border: 1px solid var(--border); border-radius: 6px; color: var(--heading); }
  .upstream-summary { color: var(--muted); font-size: 12px; }
  .repo-summary { display: block; color: var(--subtle); font-size: 13px; line-height: 1.7; margin-top: 5px; overflow-wrap: anywhere; }
  .repo-row-bottom { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 9px; margin-top: 11px; }
  .repo-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 13px; color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .repo-meta > span { display: inline-flex; gap: 4px; align-items: center; }
  .starred-label { color: var(--accent); margin-left: 3px; }
  .lists-toolbar { padding: 0 0 14px; border-bottom: 1px solid var(--border); }
  .lists-topline, .lists-topline strong { display: flex; align-items: center; gap: 8px; }
  .lists-topline { flex-wrap: wrap; gap: 8px 12px; }
  .lists-topline strong { color: var(--heading); font-size: 12px; }
  .lists-topline > span { margin-right: auto; color: var(--muted); font-size: 12px; }
  .list-options { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; }
  .list-options button { display: inline-flex; align-items: baseline; gap: 7px; max-width: 100%; padding: 5px 10px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface); color: var(--subtle); font-size: 12px; overflow-wrap: anywhere; text-align: left; }
  .list-options button:hover { border-color: var(--accent); color: var(--accent); }
  .list-options button.active { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .list-options button span { font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .active-list-info { display: flex; align-items: flex-start; justify-content: space-between; flex-wrap: wrap; gap: 8px 20px; margin-top: 10px; }
  .active-list-info p { max-width: 70ch; font-size: 12px; color: var(--subtle); overflow-wrap: anywhere; }
  .active-list-info > div { display: flex; flex-wrap: wrap; gap: 12px; }
  .danger-link { color: var(--danger); }
  .list-authorization { display: flex; justify-content: space-between; gap: 10px; flex-wrap: wrap; margin-top: 10px; font-size: 12px; color: var(--muted); }
  .list-editor { margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--border); }
  .list-editor h3 { color: var(--heading); font-size: 13px; margin-bottom: 10px; }
  .list-editor-fields { display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; }
  .list-editor label { display: block; font-size: 12px; color: var(--subtle); }
  .list-editor input { display: block; margin-top: 5px; width: 100%; font-size: 12px; }
  .inline-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  .inline-actions > button { max-width: 100%; white-space: normal; }
  .assignment-editor { padding: 16px; margin: 12px 0 16px; }
  .assignment-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .assignment-heading h2 { color: var(--heading); font-size: 14px; }
  .assignment-editor p { color: var(--subtle); font-size: 12px; overflow-wrap: anywhere; margin-top: 5px; }
  .assignment-options { display: flex; flex-wrap: wrap; gap: 8px 18px; margin-top: 13px; }
  .assignment-options label { display: flex; align-items: center; gap: 7px; color: var(--heading); font-size: 12px; }
  .assignment-options label span { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .batch-toolbar { display: flex; align-items: center; gap: 8px 12px; flex-wrap: wrap; padding: 10px 16px; border-top: 1px solid var(--border); font-size: 12px; }
  .batch-toolbar label { display: flex; align-items: center; gap: 7px; color: var(--subtle); }
  .batch-toolbar > span { color: var(--muted); font-variant-numeric: tabular-nums; margin-right: auto; }
  .repo-select-line { display: flex; align-items: flex-start; gap: 10px; }
  .repo-checkbox { margin-top: 5px; flex-shrink: 0; }
  .original-name { display: block; color: var(--muted); font-size: 12px; overflow-wrap: anywhere; margin-top: 3px; }
  .github-row-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; }
  .notes-error { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 8px 0; }
  .page-load-error { padding: 10px 16px; border-top: 1px solid var(--border); }
  .stars-pagination { flex-wrap: wrap; }
  .list-options button:focus-visible, .assignment-heading h2:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; }
  input[type="checkbox"] { accent-color: var(--accent); }
  .list-editor input, .github-search input { caret-color: var(--accent); }
  .original-name::selection, .repo-summary::selection { background: var(--accent-soft); color: var(--heading); }
  .repo-loading { padding: 18px; color: var(--muted); font-size: 12px; }
  .loading-row { display: flex; align-items: center; gap: 12px; border-top: 1px solid var(--border); padding: 20px 0; margin-top: 12px; }
  .loading-row > span { width: 32px; height: 32px; background: var(--surface-soft); border-radius: 12px; }
  .loading-row > div { flex: 1; display: flex; flex-direction: column; gap: 9px; }
  .loading-row i { height: 8px; border-radius: 3px; width: 75%; background: var(--border); }
  .loading-row i:last-child { width: 48%; }
  .activity-row { display: flex; align-items: flex-start; gap: 11px; padding: 15px 16px; border-top: 1px solid var(--border); }
  .activity-row > div { flex: 1; min-width: 0; }
  .activity-row p { font-size: 12px; color: var(--subtle); overflow-wrap: anywhere; }
  .activity-row strong { color: var(--heading); }
  .repo-link { max-width: 100%; overflow-wrap: anywhere; font-size: 12px; }
  .activity-row div > span { display: block; color: var(--muted); font-size: 12px; margin-top: 3px; }
  .github-pagination { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 12px 16px; border-top: 1px solid var(--border); font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .github-pagination > div { display: flex; gap: 8px; }
  @media (max-width: 1250px) { .github-repository-workspace { grid-template-columns: 180px minmax(0, 1fr); gap: 12px; } .github-columns.with-detail { grid-template-columns: minmax(0, 1fr); } .github-results.compact-hidden { display: none; } .github-row-actions { gap: 4px; } }
  @media (max-width: 1040px) { .github-tabs { gap: 4px; } .github-tabs button { padding: 8px 11px; } .list-editor-fields { grid-template-columns: minmax(0, 1fr); } .results-heading { flex-wrap: wrap; gap: 8px; } }
  @media (max-height: 720px) {
    .page-heading { margin-bottom: 10px; }
    .github-preview { margin-bottom: 8px; padding: 5px 10px; }
    .github-preview > span { max-width: 380px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .github-account { padding: 8px 12px; max-height: 118px; }
    .github-account.awaiting { max-height: 150px; }
    .account-line { display: grid; grid-template-columns: 34px minmax(0, 1fr); gap: 4px 8px; }
    .account-identity { min-width: 0; }
    .account-identity strong, .account-identity p { font-size: 12px; line-height: 1.4; }
    .account-actions { grid-column: 1 / -1; justify-self: end; }
    .account-actions .button { padding: 5px 9px; font-size: 12px; }
    .website-account { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 8px; }
    .website-account > div { grid-column: 1 / -1; min-width: 0; }
    .website-account p { line-height: 1.4; }
    .website-account > .button { justify-self: end; }
    .account-help { margin-top: 4px; }
    .github-search { margin: 8px 0; }
    .repo-row { padding: 10px 12px; }
    .repo-title strong { font-size: 16px; }
    .github-cache-status { padding: 6px 12px; }
    .repo-summary.local-note-summary { padding: 6px 8px; }
  }
</style>
