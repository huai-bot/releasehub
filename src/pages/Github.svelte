<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import { GITHUB_DEVICE_URL, appendGithubRepositories, filterGithubPage, findRepositoryNote, githubAccountKey, isSubscribed, repositoryEditError } from '../lib/github';
  import type { GithubActivityPage, GithubListRepositoryPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus, GithubUserList, Project, RepositoryNote } from '../lib/types';
  import { formatDate, readableError, type Tone } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import GithubAvatar from '../components/GithubAvatar.svelte';
  import GithubDetail from '../components/GithubDetail.svelte';
  import GithubFeed from '../components/GithubFeed.svelte';
  import type { GithubPreviewScenario } from '../lib/github-preview';

  let { projects, preview, initialTab = 'explore', onadd, onopen, oncopy, onnotice }: {
    projects: Project[]; preview: boolean; initialTab?: Tab;
    onadd: (repository: GithubRepository) => Promise<boolean>;
    onopen: (url: string) => void; oncopy: (text: string) => void; onnotice: (message: string, tone?: Tone) => void;
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
  let editingNote = $state<string | null>(null);
  let noteName = $state('');
  let noteDescription = $state('');
  let noteBusy = $state(false);
  let noteError = $state('');
  let catalogRequest = 0;
  let noteRequest = 0;
  let assignmentRequest = 0;
  const connected = $derived(Boolean(status?.connected && status.account));
  const visibleItems = $derived(tab === 'starred' ? stars?.items ?? [] : repositories?.items ?? []);
  const filtered = $derived(tab === 'explore' ? visibleItems : filterGithubPage(visibleItems, searchText, notes));
  const hasMore = $derived(tab === 'activity' ? activity?.hasMore ?? false : tab === 'starred' ? stars?.hasMore ?? false : repositories?.hasMore ?? false);
  const waiting = $derived(status?.login?.state === 'waiting');
  const activeList = $derived(lists.find((list) => list.id === listFilter));
  const allVisibleChecked = $derived(filtered.length > 0 && filtered.every((repository) => checkedRepositories.has(repository.fullName)));

  function stopLoginPoll() { if (loginTimer !== undefined) clearTimeout(loginTimer); loginTimer = undefined; }
  function scheduleLoginPoll() {
    stopLoginPoll();
    if (!destroyed && !preview && !authBusy && status?.login?.state === 'waiting') {
      loginTimer = setTimeout(() => { loginTimer = undefined; void refreshStatus(); }, 2_000);
    }
  }
  function clearAccountContent(resetFilters = true) {
    sessionRevision += 1; listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1;
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
    const previousAccount = githubAccountKey(status);
    const changedAccount = previousAccount !== githubAccountKey(next);
    status = next;
    // Initial account detection must not erase a search typed while it was loading.
    if (changedAccount) { clearAccountContent(Boolean(previousAccount)); void loadContent(); if (tab === 'starred') void loadLists(); }
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
  async function accountAction(action: 'github_login' | 'github_cancel_login' | 'github_logout', listsAuthorization = false) {
    if (preview || authBusy || destroyed) return;
    const request = ++statusRequest;
    stopLoginPoll(); authBusy = true; statusError = ''; statusLoading = false;
    if (action === 'github_logout') {
      // Hide private content immediately, not after the logout round-trip.
      clearAccountContent();
      if (status) status = { ...status, connected: false, account: null, login: null };
    }
    try {
      const next = action === 'github_login' ? await dispatch(action, listsAuthorization ? { lists: true } : {}) : await dispatch(action, {});
      if (!destroyed && request === statusRequest) acceptStatus(next);
    } catch (error) {
      if (!destroyed && request === statusRequest) statusError = readableError(error);
    } finally {
      if (!destroyed && request === statusRequest) {
        authBusy = false; scheduleLoginPoll();
        if (!repositories && !activity && !stars && !contentLoading) void loadContent();
        if (connected && tab === 'starred') void loadLists();
      }
    }
  }
  async function loadContent(append = false) {
    if (append && contentLoading) return;
    const request = ++listRequest;
    const session = sessionRevision;
    const requestedTab = tab;
    const requestedPage = page;
    const requestedList = listFilter;
    const cursor = append ? stars?.endCursor ?? undefined : undefined;
    const query = requestedTab === 'explore' ? searchQuery : '';
    if (!append) { repositories = null; activity = null; stars = null; starPage = 1; checkedRepositories = new Set(); }
    contentError = ''; contentLoading = false;
    if (destroyed || requestedTab === 'feed' || (requestedTab !== 'explore' && (!connected || authBusy)) || (requestedTab === 'explore' && !query)) return;
    if (append && !cursor) return;
    contentLoading = true;
    const current = () => !destroyed && session === sessionRevision && request === listRequest;
    try {
      if (requestedTab === 'activity') {
        const result = preview && import.meta.env.DEV
          ? (await import('../lib/github-preview')).createPreviewGithubActivity(requestedPage, previewScenario)
          : await dispatch('github_activity', { page: requestedPage });
        if (current()) activity = result;
      } else if (requestedTab === 'starred') {
        const result = preview && import.meta.env.DEV
          ? (await import('../lib/github-preview')).createPreviewGithubListRepositories(requestedList, cursor, previewScenario)
          : await dispatch('github_list_repositories', { listId: requestedList, ...(cursor ? { cursor } : {}) });
        if (current()) { stars = { ...result, items: append ? appendGithubRepositories(stars?.items ?? [], result.items) : result.items }; if (append) starPage += 1; }
      } else {
        const result = preview && import.meta.env.DEV
          ? (await import('../lib/github-preview')).createPreviewGithubRepositories(requestedTab, query, requestedPage, previewScenario)
          : await dispatch('github_repositories', { tab: requestedTab, query, page: requestedPage });
        if (current()) repositories = result;
      }
    } catch (error) { if (current()) contentError = readableError(error); }
    finally { if (current()) contentLoading = false; }
  }
  function resetDetail() { detailRequest += 1; selectedName = null; detail = null; detailError = ''; detailLoading = false; }
  function changeTab(next: Tab) {
    if (tab === next) return;
    tab = next; page = 1; searchText = next === 'explore' ? 'windows' : ''; searchQuery = 'windows';
    resetDetail(); assignmentRequest += 1; assignment = null; void loadContent();
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
    searchQuery = searchText.trim(); page = 1; resetDetail(); void loadContent();
  }
  function goPage(next: number) { page = Math.max(1, next); resetDetail(); void loadContent(); }
  async function selectRepository(name: string, focus = true) {
    if (pendingRepositories.has(name)) return;
    if (focus) previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    selectedName = name; detail = null; detailError = ''; detailLoading = true;
    const request = ++detailRequest;
    const session = sessionRevision;
    const current = () => !destroyed && request === detailRequest && session === sessionRevision && selectedName === name;
    if (focus) { await tick(); if (current()) detailRegion?.focus(); }
    try {
      const result = preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewGithubDetail(name, connected)
        : await dispatch('github_repository', { repository: name });
      if (current()) detail = result;
    } catch (error) { if (current()) detailError = readableError(error); }
    finally { if (current()) detailLoading = false; }
  }
  async function closeDetail() { resetDetail(); await tick(); if (previousFocus?.isConnected) previousFocus.focus(); }
  function setRepositoryPending(name: string, value: boolean) {
    const next = new Set(pendingRepositories); if (value) next.add(name); else next.delete(name); pendingRepositories = next;
  }
  function updateVisibleRepository(name: string, update: (repository: GithubRepository) => GithubRepository) {
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
  async function loadLists(): Promise<boolean> {
    const request = ++catalogRequest;
    const session = sessionRevision;
    if (!connected || destroyed || authBusy) return false;
    listsLoading = true; listsError = '';
    try {
      const result = preview && import.meta.env.DEV ? (await import('../lib/github-preview')).createPreviewGithubLists(previewScenario) : await dispatch('github_lists', {});
      if (!destroyed && request === catalogRequest && session === sessionRevision) {
        lists = result;
        if (listFilter !== 'all' && listFilter !== 'unlisted' && !result.some((list) => list.id === listFilter)) { listFilter = 'all'; if (tab === 'starred') void loadContent(); }
        return true;
      }
    } catch (error) { if (!destroyed && request === catalogRequest && session === sessionRevision) listsError = readableError(error); }
    finally { if (!destroyed && request === catalogRequest && session === sessionRevision) listsLoading = false; }
    return false;
  }
  function changeList(id: string) {
    if (id === listFilter) return;
    listFilter = id; resetDetail(); assignmentRequest += 1; assignment = null; void loadContent();
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
      lists = [...lists.filter((list) => list.id !== saved.id), saved]; editingList = null;
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
      lists = lists.filter((list) => list.id !== selected.id); listDelete = null;
      if (listFilter === selected.id) { listFilter = 'all'; void loadContent(); }
      else if (listFilter === 'unlisted') void loadContent();
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
    assignmentReady = false; assignmentLoading = true; assignmentError = '';
    await tick(); document.getElementById('github-assignment-title')?.focus();
    const current = () => !destroyed && session === sessionRevision && request === assignmentRequest && assignment !== null;
    try {
      if (!await loadLists()) throw new Error(listsError || '分类列表尚未读到，请重试。');
      if (!current()) return;
      if (mode === 'replace') {
        const result = preview && import.meta.env.DEV
          ? { listIds: (await import('../lib/github-preview')).createPreviewRepositoryLists(names[0]) }
          : await dispatch('github_repository_lists', { repository: names[0] });
        if (!current() || !assignment) return;
        if (result.listIds.some((id) => !lists.some((list) => list.id === id))) throw new Error('GitHub 分类刚发生了变化，请重新读取归属。');
        assignment = { ...assignment, listIds: result.listIds };
      }
      if (current()) assignmentReady = true;
    } catch (error) { if (current()) assignmentError = readableError(error); }
    finally { if (current()) assignmentLoading = false; }
  }
  function toggleAssignmentList(id: string) {
    if (!assignment) return;
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
      assignment = null; checkedRepositories = new Set();
      onnotice(result.updated ? `已同步 ${result.updated} 个仓库的分类。` : '所选仓库的分类已经一致。', 'success');
      await loadLists(); if (tab === 'starred') await loadContent();
    } catch (error) {
      if (!destroyed && session === sessionRevision) { assignmentError = readableError(error); void loadLists(); if (tab === 'starred') void loadContent(); }
    } finally { if (!destroyed && session === sessionRevision) assignmentBusy = false; }
  }
  async function loadNotes() {
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
      editingNote = null; onnotice(result ? '本地备注已保存。' : '本地备注已清除。', 'success');
      await loadNotes();
    } catch (error) { if (!destroyed && session === sessionRevision) noteError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) noteBusy = false; }
  }
  function changePreview() {
    stopLoginPoll(); statusRequest += 1; clearAccountContent(); status = null;
    void refreshStatus(true); void loadContent(); void loadNotes();
  }
  onMount(() => {
    void refreshStatus(true); void loadContent(); void loadNotes();
    return () => { destroyed = true; statusRequest += 1; listRequest += 1; detailRequest += 1; catalogRequest += 1; assignmentRequest += 1; noteRequest += 1; sessionRevision += 1; stopLoginPoll(); };
  });
</script>

<div class="page-heading"><div><h1>GitHub</h1><p>浏览仓库、整理 Stars 与分类，用备注记住想跟进的软件。</p></div><span class="quiet-label"><Icon name="github" size={16} />日常管理</span></div>

{#if preview}
  <div class="github-preview"><span><strong>GitHub 界面示例</strong> · 账号、仓库与数量均为演示，不访问真实账号。</span><label>示例状态<select aria-label="GitHub 示例状态" bind:value={previewScenario} onchange={changePreview}><option value="connected">已连接</option><option value="anonymous">未登录</option><option value="waiting">等待短码授权</option><option value="unavailable">缺少登录组件</option><option value="empty">空列表</option><option value="error">读取失败</option></select></label></div>
{/if}

<section class="surface github-account" aria-label="GitHub 账号">
  <div class="account-line">
    <GithubAvatar url={status?.account?.avatarUrl} name={status?.account?.login ?? 'GitHub'} />
    <div class="account-identity" aria-live="polite">
      {#if statusLoading}<strong>正在读取账号状态…</strong><p>公开仓库仍可搜索和浏览。</p>
      {:else if connected && status?.account}<strong>{status.account.name || status.account.login}{#if preview}<span class="status-badge warning">示例账号</span>{/if}</strong><p>@{status.account.login} · 已连接 GitHub CLI</p>
      {:else if waiting}<strong>等待你在 GitHub 完成授权</strong><p>短码只用于这次登录；完成后本页会自动更新。</p>
      {:else if status?.available === false}<strong>没有找到 GitHub 登录组件</strong><p>公开仓库仍可查看。请使用带 GitHub CLI 的完整发布包。</p>
      {:else}<strong>连接你的 GitHub 账号</strong><p>公开仓库无需登录；我的仓库、Stars 和关注动态使用这个账号。</p>{/if}
    </div>
    <div class="account-actions"><button class="icon-button" aria-label="刷新 GitHub 账号状态" disabled={authBusy || statusLoading} onclick={() => void refreshStatus(true)}><Icon name="refresh" size={16} /></button>{#if waiting}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_cancel_login')}>{authBusy ? '正在取消' : '取消登录'}</button>{:else if connected}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_logout')}><Icon name="logout" size={15} />仅断开追新</button>{:else}<button class="button primary" disabled={authBusy || statusLoading || status?.available === false || preview} onclick={() => void accountAction('github_login')}><Icon name={authBusy ? 'loader' : 'github'} class={authBusy ? 'spinning' : ''} size={16} />{authBusy ? '正在连接' : '登录 GitHub'}</button>{/if}</div>
  </div>
  <p class="account-note">通过官方 GitHub CLI 连接账号。Stars 与分类同步到 GitHub，备注保存在本地；授权保存在本机凭据库。分类管理首次使用时需额外授权。</p>
  {#if waiting}<div class="device-login"><div><label for="github-device-code">一次性登录短码</label><div class="code-line"><input id="github-device-code" class="mono" readonly value={status?.login?.userCode ?? '正在获取短码…'} /><button class="button secondary" disabled={!status?.login?.userCode} onclick={() => oncopy(status!.login!.userCode!)}><Icon name="copy" size={15} />复制短码</button></div></div><div><button class="button primary" disabled={!status?.login?.userCode || preview} onclick={() => onopen(GITHUB_DEVICE_URL)}>打开 GitHub 授权页<Icon name="external" size={15} /></button><p>在 github.com/login/device 核对账号，再输入短码。</p></div></div>{/if}
  {#if statusError || status?.error || status?.login?.state === 'error'}<p class="field-error" role="alert">{statusError || status?.error || status?.login?.message || '登录未完成，请重新登录。'}</p>{/if}
</section>

<div class="github-tabs" role="tablist" aria-label="GitHub 内容">{#each tabs as item, index}<button id={`github-tab-${item.id}`} role="tab" aria-selected={tab === item.id} aria-controls="github-results" tabindex={tab === item.id ? 0 : -1} class:active={tab === item.id} onclick={() => changeTab(item.id)} onkeydown={(event) => tabKey(event, index)}><Icon name={item.icon} size={16} />{item.label}</button>{/each}</div>

{#if tab === 'feed'}
  <div id="github-results" role="tabpanel" aria-labelledby="github-tab-feed" tabindex="0"><GithubFeed {preview} {onopen} {onadd} {onnotice} /></div>
{:else}
  {#if tab === 'starred' && connected}
    <section class="lists-toolbar" aria-label="GitHub 分类">
      <div class="lists-topline"><strong><Icon name="folder" size={16} />GitHub 分类</strong><span>{listsLoading ? '正在读取分类…' : `${lists.length} 个分类`}</span><button class="text-button" disabled={listsLoading} onclick={() => void loadLists()}><Icon name="refresh" size={14} />刷新分类</button><button class="button secondary small" disabled={authBusy || listBusy || preview} onclick={() => editUserList()}><Icon name="plus" size={14} />新建分类</button></div>
      <div class="list-options" aria-label="选择 Stars 分类"><button class:active={listFilter === 'all'} aria-pressed={listFilter === 'all'} onclick={() => changeList('all')}>全部 Stars</button><button class:active={listFilter === 'unlisted'} aria-pressed={listFilter === 'unlisted'} onclick={() => changeList('unlisted')}>未分类</button>{#each lists as list (list.id)}<button class:active={listFilter === list.id} aria-pressed={listFilter === list.id} title={list.description ?? list.name} onclick={() => changeList(list.id)}>{list.name}<span>{list.itemCount}</span></button>{/each}</div>
      {#if activeList}<div class="active-list-info"><p>{activeList.description || '这个分类还没有说明。'}</p><div><button class="text-button" disabled={listBusy || preview} onclick={() => editUserList(activeList)}><Icon name="edit" size={13} />改名与说明</button><button class="text-button" onclick={() => onopen(activeList.url)}>官网分类<Icon name="external" size={13} /></button><button class="text-button danger-link" disabled={listBusy || preview} onclick={() => { listDelete = activeList ?? null; listEditError = ''; }}><Icon name="trash" size={13} />删除分类</button></div></div>{/if}
      {#if !lists.length && !listsLoading && !listsError}<p class="field-hint">新建第一个分类，开始整理 Stars。同一个仓库可放入多个分类。</p>{/if}
      {#if listsError}<p class="field-error" role="alert">{listsError}</p>{/if}
      <div class="list-authorization"><span>分类与 GitHub Lists 同步；批量添加会保留已有归属。</span><button class="text-button" disabled={preview || authBusy || waiting || assignmentBusy || listBusy} onclick={() => void accountAction('github_login', true)}>授权分类管理<Icon name="external" size={13} /></button></div>
      {#if editingList}<form class="list-editor" onsubmit={saveUserList}><h3>{editingList.id ? '改名与说明' : '新建 GitHub 分类'}</h3><div class="list-editor-fields"><label>分类名称<input bind:value={editingList.name} maxlength="100" required disabled={listBusy} placeholder="例如：桌面工具" /></label><label>分类说明 <span class="optional">可选</span><input bind:value={editingList.description} maxlength="1000" disabled={listBusy} placeholder="这个分类收集哪些仓库" /></label></div>{#if listEditError}<p class="field-error" role="alert">{listEditError}</p>{/if}<div class="inline-actions"><button class="button secondary small" type="button" disabled={listBusy} onclick={() => editingList = null}>取消</button><button class="button primary small" type="submit" disabled={listBusy || preview || !editingList.name.trim()}>{listBusy ? '正在保存并回读' : editingList.id ? '保存分类到 GitHub' : '创建分类到 GitHub'}</button></div></form>{/if}
    </section>
  {/if}
  {#if tab !== 'activity'}
    <form class="github-search" onsubmit={search}><label for="github-search">{tab === 'explore' ? '搜索 GitHub 仓库' : tab === 'starred' ? '筛选已加载 Stars' : '筛选本页'}</label><div class="search-line"><div class="input-leading"><Icon name={tab === 'explore' ? 'search' : 'filter'} size={17} /><input id="github-search" bind:value={searchText} maxlength="256" placeholder={tab === 'explore' ? '关键词、作者/仓库，或 topic:windows' : '仓库名、备注、说明或语言'} disabled={tab !== 'explore' && !connected} />{#if searchText}<button type="button" class="icon-button" aria-label="清空仓库搜索" onclick={() => searchText = ''}><Icon name="close" size={14} /></button>{/if}</div>{#if tab === 'explore'}<button class="button primary" type="submit">搜索</button>{/if}</div><p class="field-hint">{tab === 'explore' ? '使用 GitHub 搜索语法；未登录时可搜索公开仓库。' : tab === 'starred' ? `按仓库信息和本地备注筛选当前分类的所有已加载结果。${hasMore ? '继续加载可扩大搜索范围。' : stars ? '当前分类已全部加载。' : ''}` : '按仓库信息和本地备注筛选当前页；翻页可查看其余仓库。'}</p></form>
  {:else}<p class="activity-note"><Icon name="clock" size={15} />你关注账号的近期活动，最多展示最近 30 天、300 条。个性化推荐请查看“首页动态”。</p>{/if}
  {#if notesError}<div class="notes-error"><p class="field-error">本地备注读取失败：{notesError}</p><button class="text-button" onclick={() => void loadNotes()}>重试备注</button></div>{/if}
  {#if assignment}<section class="assignment-editor surface" aria-label="整理 GitHub 分类"><form onsubmit={saveAssignment}><div class="assignment-heading"><h2 id="github-assignment-title" tabindex="-1">{assignment.mode === 'replace' ? '设置仓库分类' : assignment.mode === 'add' ? '批量添加到分类' : '批量从分类移出'}</h2><button class="icon-button" type="button" aria-label="关闭分类整理" disabled={assignmentBusy} onclick={closeAssignment}><Icon name="close" size={16} /></button></div><p>{assignment.repositories.length === 1 ? assignment.repositories[0] : `已选择 ${assignment.repositories.length} 个仓库`}</p><p class="field-hint">{assignment.mode === 'replace' ? '勾选这个仓库所属的全部分类，取消所有勾选会将其设为未分类。' : assignment.mode === 'add' ? '添加所选分类，保留每个仓库原有的其它分类。' : '仅移出勾选的分类，其它分类和 Star 保留。'}</p>{#if assignmentLoading}<p role="status">正在读取现有分类归属…</p>{:else}<div class="assignment-options">{#each lists as list (list.id)}<label><input type="checkbox" checked={assignment.listIds.includes(list.id)} disabled={assignmentBusy || !assignmentReady} onchange={() => toggleAssignmentList(list.id)} />{list.name}<span>{list.itemCount}</span></label>{:else}<p>还没有可用分类，请先新建分类。</p>{/each}</div>{/if}{#if assignmentError}<p class="field-error" role="alert">{assignmentError}</p>{/if}{#if !assignmentReady && !assignmentLoading}<button class="text-button" type="button" onclick={() => { if (assignment) void openAssignment(assignment.repositories, assignment.mode); }}>重新读取归属</button>{/if}<div class="inline-actions"><button class="button secondary small" type="button" disabled={assignmentBusy} onclick={closeAssignment}>取消</button><button class="button primary small" type="submit" disabled={preview || assignmentBusy || !assignmentReady || (assignment.mode !== 'replace' && !assignment.listIds.length)}>{assignmentBusy ? '正在同步并回读' : '确认同步分类'}</button></div></form></section>{/if}

<div class="github-columns" class:with-detail={selectedName !== null}>
  <div class="surface github-results" class:compact-hidden={selectedName !== null} id="github-results" role="tabpanel" tabindex="0" aria-labelledby={`github-tab-${tab}`} aria-busy={contentLoading}>
    {#if tab !== 'explore' && !connected}
      <div class="empty-state spacious"><Icon name={tab === 'starred' ? 'star' : tab === 'owned' ? 'user' : 'activity'} size={29} /><h3>登录后查看{tab === 'starred' ? '你的 Stars' : tab === 'owned' ? '自己的仓库' : 'GitHub 动态'}</h3><p>上方连接账号后会载入这里。也可以先搜索公开仓库。</p><button class="button secondary" onclick={() => changeTab('explore')}>先浏览公开仓库</button></div>
    {:else if contentLoading && !stars}
      <div class="repo-loading" role="status"><p>正在读取{tab === 'activity' ? '近期活动' : '仓库列表'}…</p>{#each [1, 2, 3] as row}<div class="loading-row" aria-hidden="true"><span></span><div><i></i><i></i></div></div>{/each}</div>
    {:else if contentError && !stars}
      <div class="empty-state spacious"><Icon name="alert" size={27} /><h3>暂时没有读到{tab === 'activity' ? '活动' : '仓库'}</h3><p class="break-word" role="alert">{contentError}</p><button class="button secondary" onclick={() => void loadContent()}>重试这一页</button></div>
    {:else if tab === 'explore' && !searchQuery}
      <div class="empty-state spacious"><Icon name="search" size={29} /><h3>从一个关键词开始</h3><p>输入项目名称、作者或 GitHub 搜索条件，查找你想关注的软件。</p></div>
    {:else if tab === 'activity'}
      <div class="results-heading"><h2>近期活动</h2><span>第 {page} 页</span><button class="icon-button" aria-label="刷新 GitHub 活动" onclick={() => void loadContent()}><Icon name="refresh" size={15} /></button></div>
      {#each activity?.items ?? [] as item (item.id)}<article class="activity-row"><GithubAvatar url={item.avatarUrl} name={item.actor} /><div><p><strong>{item.actor}</strong> {item.summary}</p><button class="text-button repo-link" onclick={() => void selectRepository(item.repository)}>{item.repository}<Icon name="chevronRight" size={13} /></button><span>{formatDate(item.createdAt, true)}</span></div><button class="icon-button" aria-label={`在 GitHub 查看 ${item.actor} 的活动`} onclick={() => onopen(item.url)}><Icon name="external" size={15} /></button></article>{:else}<div class="empty-state"><Icon name="activity" size={27} /><h3>这一页没有近期活动</h3><p>GitHub 不一定会立即提供动态，可稍后刷新，或返回上一页。</p></div>{/each}
    {:else}
      <div class="results-heading"><h2>{tab === 'explore' ? '搜索结果' : tab === 'owned' ? '我的仓库' : activeList?.name ?? (listFilter === 'unlisted' ? '未分类 Stars' : '我的 Stars')}</h2><span>{tab === 'explore' && repositories?.totalCount !== null && repositories?.totalCount !== undefined ? `约 ${repositories.totalCount.toLocaleString()} 个结果` : tab === 'starred' ? `显示 ${filtered.length} · 已加载 ${stars?.items.length ?? 0} / ${stars?.totalCount ?? 0}` : `本页 ${filtered.length} / ${repositories?.items.length ?? 0} 个`}</span><button class="icon-button" aria-label="刷新仓库列表" disabled={contentLoading || assignmentBusy} onclick={() => { void loadContent(); if (tab === 'starred') void loadLists(); void loadNotes(); }}><Icon name="refresh" size={15} /></button></div>
      {#if tab === 'starred' && visibleItems.length}<div class="batch-toolbar"><label><input type="checkbox" aria-label="选择筛选结果" checked={allVisibleChecked} disabled={assignmentBusy} onchange={toggleVisibleRepositories} />选择当前结果</label><span>已选 {checkedRepositories.size} / 100</span><button class="button secondary small" disabled={!checkedRepositories.size || assignmentBusy || preview} onclick={() => void openAssignment([...checkedRepositories], 'add')}>添加到分类</button><button class="button secondary small" disabled={!checkedRepositories.size || assignmentBusy || preview} onclick={() => void openAssignment([...checkedRepositories], 'remove')}>从分类移出</button>{#if checkedRepositories.size}<button class="text-button" disabled={assignmentBusy} onclick={() => checkedRepositories = new Set()}>清空选择</button>{/if}</div>{/if}
      {#each filtered as repository (repository.id)}
        {@const note = findRepositoryNote(notes, repository.fullName)}
        <article class="repo-row" class:selected={selectedName === repository.fullName}>
          <div class="repo-select-line">{#if tab === 'starred'}<input type="checkbox" class="repo-checkbox" aria-label={`选择 ${repository.fullName}`} checked={checkedRepositories.has(repository.fullName)} disabled={assignmentBusy || pendingRepositories.has(repository.fullName)} onchange={() => toggleRepository(repository.fullName)} />{/if}<button class="repo-main" aria-label={`查看 ${repository.fullName} 详情`} aria-pressed={selectedName === repository.fullName} onclick={() => void selectRepository(repository.fullName)}><span class="repo-title"><Icon name={repository.private ? 'private' : 'readme'} size={16} /><strong>{note?.name || repository.fullName}</strong>{#if note?.name}<span class="note-tag">备注</span>{/if}{#if repository.private}<span class="status-badge neutral">私有</span>{/if}{#if repository.archived}<span class="status-badge warning">已归档</span>{/if}</span>{#if note?.name}<span class="original-name">{repository.fullName}</span>{/if}<span class="repo-summary">{note?.description || repository.description || '尚未填写仓库说明'}</span></button></div>
          <div class="repo-row-bottom"><div class="repo-meta"><span><Icon name="star" size={13} />{repository.stars.toLocaleString()}{#if repository.starred === true}<span class="starred-label">已 Star</span>{/if}</span>{#if repository.language}<span>{repository.language}</span>{/if}<span>更新 {formatDate(repository.updatedAt)}</span></div><div class="github-row-actions"><button class="icon-button" aria-label={`编辑 ${repository.fullName} 本地备注`} disabled={preview} onclick={() => editNote(repository.fullName)}><Icon name="notes" size={15} /></button>{#if tab === 'starred'}<button class="icon-button" aria-label={`设置 ${repository.fullName} 分类`} disabled={preview || assignmentBusy} onclick={() => void openAssignment([repository.fullName], 'replace')}><Icon name="folder" size={15} /></button>{/if}<button class="button secondary small" disabled={preview || repository.private || isSubscribed(repository.fullName, projects) || addingRepositories.has(repository.fullName)} title={repository.private ? '私有仓库暂不加入软件库' : '订阅这个软件的版本更新'} onclick={() => void addRepository(repository)}><Icon name={addingRepositories.has(repository.fullName) ? 'loader' : isSubscribed(repository.fullName, projects) ? 'check' : 'plus'} class={addingRepositories.has(repository.fullName) ? 'spinning' : ''} size={14} />{isSubscribed(repository.fullName, projects) ? '已订阅' : addingRepositories.has(repository.fullName) ? '正在加入' : '加入软件库'}</button></div></div>
        </article>
      {:else}<div class="empty-state"><Icon name={tab === 'starred' ? 'star' : 'search'} size={27} /><h3>{tab === 'starred' ? searchText.trim() ? '已加载的 Stars 中没有匹配结果' : hasMore ? '这批 Stars 没有符合分类的仓库' : '这个分类还没有仓库' : tab === 'owned' && searchText.trim() ? '本页没有匹配的仓库' : '这一页没有找到仓库'}</h3><p>{tab === 'starred' ? hasMore ? '继续加载下一批，搜索会保留并覆盖更多 Stars。' : '试试其它分类，或调整搜索关键词。' : tab === 'owned' && searchText.trim() ? '可以清空筛选，或继续翻页。' : '换一个关键词，或稍后刷新。'}</p>{#if tab !== 'explore' && searchText.trim()}<button class="button secondary" onclick={() => searchText = ''}>{tab === 'starred' ? '清空 Stars 筛选' : '清空本页筛选'}</button>{/if}</div>{/each}
    {/if}
    {#if tab === 'starred' && stars}<div class="github-pagination stars-pagination"><span>已加载 {starPage} 批 · {stars.items.length} / {stars.totalCount} 个{!hasMore ? ' · 已全部加载' : ''}</span><button class="button secondary small" disabled={!hasMore || contentLoading || assignmentBusy} onclick={() => void loadContent(true)}><Icon name={contentLoading ? 'loader' : 'plus'} class={contentLoading ? 'spinning' : ''} size={14} />{contentLoading ? '正在加载' : hasMore ? '继续加载 Stars' : '已全部加载'}</button></div>{#if contentError}<div class="page-load-error"><p class="field-error" role="alert">{contentError}</p><button class="text-button" onclick={() => void loadContent(true)}>重试下一批</button></div>{/if}
    {:else if !contentLoading && !contentError && (tab === 'explore' || connected) && (repositories || activity)}<div class="github-pagination"><span>第 {page} 页{tab === 'owned' ? ' · 筛选当前页' : ''}</span><div><button class="button secondary small" disabled={page <= 1} onclick={() => goPage(page - 1)}>上一页</button><button class="button secondary small" disabled={!hasMore} onclick={() => goPage(page + 1)}>下一页</button></div></div>{/if}
  </div>
  {#if selectedName}<div class="detail-region" bind:this={detailRegion} tabindex="-1"><GithubDetail name={selectedName} {detail} note={findRepositoryNote(notes, selectedName)} loading={detailLoading} error={detailError} {connected} {preview} pending={pendingRepositories.has(selectedName)} subscribed={isSubscribed(selectedName, projects)} adding={addingRepositories.has(selectedName)} onclose={() => void closeDetail()} onretry={() => { if (selectedName) void selectRepository(selectedName, false); }} onstar={(repository) => void setStar(repository)} onedit={editRepository} onnote={editNote} onlists={(repository) => void openAssignment([repository.fullName], 'replace')} onadd={(repository) => void addRepository(repository)} {onopen} /></div>{/if}
</div>
{/if}

{#if listDelete}
  <Modal title="删除 GitHub 分类" description={`删除“${listDelete.name}”及其分类归属，仓库的 Star 和其它分类会保留。`} onclose={() => { if (!listBusy) listDelete = null; }}><div class="modal-form">{#if listEditError}<p class="field-error" role="alert">{listEditError}</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={listBusy} onclick={() => listDelete = null}>保留分类</button><button class="button danger" disabled={listBusy || preview} onclick={() => void deleteUserList()}>{listBusy ? '正在删除并回读' : '确认删除分类'}</button></div></div></Modal>
{/if}

{#if editingNote}
  <Modal title="本地备注" description={editingNote} onclose={() => { if (!noteBusy) editingNote = null; }}><form class="modal-form" onsubmit={saveNote}><p class="field-hint">备注在追新中优先显示和搜索，GitHub 原仓库名仍会保留。两项都留空即可清除备注。</p><label>备注名<input bind:value={noteName} maxlength="80" disabled={noteBusy} data-initial-focus placeholder="给这个仓库起个好记的名字" /></label><label>备注说明<input bind:value={noteDescription} maxlength="240" disabled={noteBusy} placeholder="记录用途或下次查看的线索" /></label>{#if noteError}<p class="field-error" role="alert">{noteError}</p>{/if}<div class="modal-footer"><button class="button secondary" type="button" disabled={noteBusy} onclick={() => editingNote = null}>取消</button><button class="button primary" type="submit" disabled={noteBusy || preview}>{noteBusy ? '正在保存' : '保存本地备注'}</button></div></form></Modal>
{/if}

{#if editing}
  <Modal title="修改仓库说明与主页" description={`更新 GitHub 上 ${editing.fullName} 的仓库资料。`} onclose={() => { if (!editBusy) editing = null; }}>
    <form class="modal-form" onsubmit={saveRepository}><label>仓库说明<textarea bind:value={description} maxlength="350" rows="3" disabled={editBusy} data-initial-focus placeholder="用一句话说明这个仓库做什么；留空可移除"></textarea></label><label>主页地址 <span class="optional">可选</span><input type="url" bind:value={homepage} maxlength="2000" disabled={editBusy} placeholder="https://example.com" /></label><p class="field-hint">填写完整 HTTPS 地址，留空移除主页；可在 GitHub 原页访问站外主页。</p><label class="confirmation-check"><input type="checkbox" bind:checked={editConfirmed} disabled={editBusy || preview} required />我确认将以上说明与主页保存到 GitHub。</label>{#if preview}<p class="field-hint">这是示例，不能提交到真实仓库。</p>{/if}{#if editError}<p class="field-error" role="alert">{editError}</p>{/if}<div class="modal-footer"><button class="button secondary" type="button" disabled={editBusy} onclick={() => editing = null}>取消</button><button class="button primary" type="submit" disabled={editBusy || !editConfirmed || preview}><Icon name={editBusy ? 'loader' : 'check'} class={editBusy ? 'spinning' : ''} size={16} />{editBusy ? '正在保存' : '确认保存到 GitHub'}</button></div></form>
  </Modal>
{/if}

<style>
  .github-preview { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px 20px; color: var(--warning); background: var(--warning-bg); border: 1px solid var(--border); border-radius: 6px; margin-bottom: 14px; padding: 8px 12px; font-size: 11px; }
  .github-preview label { display: flex; gap: 8px; align-items: center; }
  .github-preview select { padding: 4px 8px; font-size: 11px; }
  .github-account { padding: 16px 18px; }
  .account-line { display: flex; align-items: center; gap: 11px; }
  .account-identity { flex: 1; min-width: 0; }
  .account-identity strong { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; color: var(--heading); font-size: 13px; overflow-wrap: anywhere; }
  .account-identity p { color: var(--muted); font-size: 11.5px; margin-top: 2px; }
  .account-actions { display: flex; gap: 9px; align-items: center; flex-shrink: 0; }
  .account-note { color: var(--muted); font-size: 11px; margin-top: 13px; padding-top: 11px; border-top: 1px solid var(--border); line-height: 1.8; }
  .device-login { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 14px 22px; border-top: 1px solid var(--border); padding-top: 14px; margin-top: 14px; }
  .device-login label { display: block; font-size: 11px; color: var(--subtle); margin-bottom: 5px; }
  .code-line { display: flex; gap: 8px; align-items: center; }
  .code-line input { width: 178px; padding: 7px 10px; font-size: 17px; font-weight: 650; letter-spacing: .04em; }
  .device-login p { color: var(--muted); font-size: 11px; margin-top: 6px; }
  .github-tabs { display: flex; align-items: center; gap: 20px; margin-top: 18px; border-bottom: 1px solid var(--border); }
  .github-tabs button { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 12px; padding: 11px 2px; border-bottom: 2px solid transparent; }
  .github-tabs button:hover { color: var(--accent); }
  .github-tabs button.active { color: var(--accent); border-bottom-color: var(--accent); font-weight: 650; }
  .github-search { margin: 16px 0; }
  .github-search > label { display: block; font-size: 11.5px; color: var(--subtle); margin-bottom: 6px; }
  .search-line { display: flex; gap: 9px; }
  .search-line .input-leading { flex: 1; background: var(--surface); }
  .search-line input { font-size: 12px; }
  .search-line .icon-button { margin-right: 4px; }
  .github-search .field-hint { margin-top: 6px; }
  .activity-note { display: flex; align-items: flex-start; gap: 7px; color: var(--muted); font-size: 11.5px; padding: 16px 0; }
  .activity-note :global(svg) { margin-top: 3px; }
  .github-columns { display: grid; grid-template-columns: minmax(0, 1fr); gap: 18px; align-items: start; }
  .github-columns.with-detail { grid-template-columns: minmax(310px, .9fr) minmax(380px, 1.1fr); }
  .github-results, .detail-region { min-width: 0; }
  .detail-region:focus { outline: none; }
  .detail-region:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; border-radius: 8px; }
  .results-heading { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: var(--surface-soft); }
  .results-heading h2 { font-size: 12px; }
  .results-heading > span { margin-left: auto; color: var(--muted); font-size: 11px; }
  .repo-row { padding: 14px 16px; border-top: 1px solid var(--border); }
  .repo-row:hover { background: var(--surface-soft); }
  .repo-row.selected { background: var(--accent-soft); }
  .repo-main { display: block; width: 100%; min-width: 0; padding: 0; text-align: left; }
  .repo-title { display: flex; align-items: center; gap: 7px; color: var(--accent); flex-wrap: wrap; }
  .repo-title strong { font-size: 13px; font-weight: 600; overflow-wrap: anywhere; }
  .repo-summary { display: block; color: var(--subtle); font-size: 12px; line-height: 1.7; margin-top: 5px; overflow-wrap: anywhere; }
  .repo-row-bottom { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 9px; margin-top: 11px; }
  .repo-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 13px; color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .repo-meta > span { display: inline-flex; gap: 4px; align-items: center; }
  .starred-label { color: var(--accent); margin-left: 3px; }
  .lists-toolbar { margin-top: 16px; padding: 14px 0; border-bottom: 1px solid var(--border); }
  .lists-topline, .lists-topline strong { display: flex; align-items: center; gap: 8px; }
  .lists-topline { flex-wrap: wrap; gap: 8px 12px; }
  .lists-topline strong { color: var(--heading); font-size: 12px; }
  .lists-topline > span { margin-right: auto; color: var(--muted); font-size: 11px; }
  .list-options { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 12px; }
  .list-options button { display: inline-flex; align-items: baseline; gap: 7px; max-width: 100%; padding: 5px 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface); color: var(--subtle); font-size: 12px; overflow-wrap: anywhere; text-align: left; }
  .list-options button:hover { border-color: var(--accent); color: var(--accent); }
  .list-options button.active { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .list-options button span { font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .active-list-info { display: flex; align-items: flex-start; justify-content: space-between; flex-wrap: wrap; gap: 8px 20px; margin-top: 10px; }
  .active-list-info p { max-width: 70ch; font-size: 12px; color: var(--subtle); overflow-wrap: anywhere; }
  .active-list-info > div { display: flex; flex-wrap: wrap; gap: 12px; }
  .danger-link { color: var(--danger); }
  .list-authorization { display: flex; justify-content: space-between; gap: 10px; flex-wrap: wrap; margin-top: 10px; font-size: 11px; color: var(--muted); }
  .list-editor { margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--border); }
  .list-editor h3 { color: var(--heading); font-size: 13px; margin-bottom: 10px; }
  .list-editor-fields { display: grid; grid-template-columns: minmax(160px, .6fr) minmax(220px, 1fr); gap: 12px; }
  .list-editor label { display: block; font-size: 12px; color: var(--subtle); }
  .list-editor input { display: block; margin-top: 5px; width: 100%; font-size: 12px; }
  .inline-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  .assignment-editor { padding: 16px; margin: 12px 0 16px; }
  .assignment-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .assignment-heading h2 { color: var(--heading); font-size: 14px; }
  .assignment-editor p { color: var(--subtle); font-size: 12px; overflow-wrap: anywhere; margin-top: 5px; }
  .assignment-options { display: flex; flex-wrap: wrap; gap: 8px 18px; margin-top: 13px; }
  .assignment-options label { display: flex; align-items: center; gap: 7px; color: var(--heading); font-size: 12px; }
  .assignment-options label span { color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .batch-toolbar { display: flex; align-items: center; gap: 8px 12px; flex-wrap: wrap; padding: 10px 16px; border-top: 1px solid var(--border); font-size: 11px; }
  .batch-toolbar label { display: flex; align-items: center; gap: 7px; color: var(--subtle); }
  .batch-toolbar > span { color: var(--muted); font-variant-numeric: tabular-nums; margin-right: auto; }
  .repo-select-line { display: flex; align-items: flex-start; gap: 10px; }
  .repo-checkbox { margin-top: 5px; flex-shrink: 0; }
  .original-name { display: block; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; margin-top: 3px; }
  .note-tag { color: var(--muted); font-size: 10px; font-weight: 400; }
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
  .loading-row > span { width: 32px; height: 32px; background: var(--surface-soft); border-radius: 6px; }
  .loading-row > div { flex: 1; display: flex; flex-direction: column; gap: 9px; }
  .loading-row i { height: 8px; border-radius: 3px; width: 75%; background: var(--border); }
  .loading-row i:last-child { width: 48%; }
  .activity-row { display: flex; align-items: flex-start; gap: 11px; padding: 15px 16px; border-top: 1px solid var(--border); }
  .activity-row > div { flex: 1; min-width: 0; }
  .activity-row p { font-size: 12px; color: var(--subtle); overflow-wrap: anywhere; }
  .activity-row strong { color: var(--heading); }
  .repo-link { max-width: 100%; overflow-wrap: anywhere; font-size: 12px; }
  .activity-row div > span { display: block; color: var(--muted); font-size: 11px; margin-top: 3px; }
  .github-pagination { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 12px 16px; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .github-pagination > div { display: flex; gap: 8px; }
  @media (max-width: 1250px) { .github-columns.with-detail { grid-template-columns: minmax(0, 1fr); } .github-results.compact-hidden { display: none; } }
  @media (max-width: 1040px) { .account-line { flex-wrap: wrap; } .account-actions { margin-left: 45px; } .github-tabs { gap: 8px 14px; flex-wrap: wrap; } .list-editor-fields { grid-template-columns: minmax(0, 1fr); } .results-heading { flex-wrap: wrap; gap: 8px; } }
</style>
