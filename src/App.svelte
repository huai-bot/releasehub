<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import TitleBar from './components/TitleBar.svelte';
  import BrandMark from './components/BrandMark.svelte';
  import { desktopAvailable, dispatch, watchChanges, type Action, type Actions } from './lib/bridge';
  import { applyDownloadProgress, mergeSnapshotDownloads } from './lib/download-state';
  import type { AddProjectInput, AgentIntegration, AssetPreference, BackupRecord, Channel, DownloadRecord, GithubRepository, InstallPreview, ManagedVersion, ManagementMode, Project, ProjectChanges, ProxySettings, RepositoryNote, Settings, Snapshot, WorkspacePage } from './lib/types';
  import { canActivateManagedVersion, formatBytes, parseRepository, projectPresentation, readableError, safeGithubUrl, type Page, type Tone } from './lib/ui';
  import ManagedVersions from './components/ManagedVersions.svelte';
  import InstallConfirmation from './components/InstallConfirmation.svelte';
  import Icon from './components/Icon.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Modal from './components/Modal.svelte';
  import ProjectDetail from './components/ProjectDetail.svelte';
  import Home from './pages/Home.svelte';
  import Library from './pages/Library.svelte';
  import Downloads from './pages/Downloads.svelte';
  import Github from './pages/Github.svelte';
  import Connections from './pages/Connections.svelte';
  import { isSubscribed } from './lib/github';
  import SettingsPage from './pages/Settings.svelte';

  type DialogState = { kind: 'external'; projects: Project[]; retry?: DownloadRecord } | { kind: 'add' } | { kind: 'install'; download: DownloadRecord } | { kind: 'managed'; projectId: string } | { kind: 'mark'; project: Project } | { kind: 'note'; project: Project } | { kind: 'remove'; project: Project } | { kind: 'import' } | { kind: 'deleteDownload'; download: DownloadRecord } | { kind: 'backup'; operation: 'restore' | 'delete'; backup: BackupRecord };
  interface Toast { id: number; message: string; tone: Tone }
  let snapshot = $state<Snapshot | null>(null);
  let page = $state<Page>('home');
  let query = $state('');
  let selectedId = $state<string | null>(null);
  let detailRevision = $state(0);
  let detailTab = $state<'notes' | 'manage'>('notes');
  let preview = $state(false);
  let loading = $state(true);
  let refreshing = $state(false);
  let startupError = $state('');
  let operationError = $state('');
  let pending = $state(new Set<string>());
  let toasts = $state<Toast[]>([]);
  let modal = $state<DialogState | null>(null);
  let formError = $state('');
  let managedVersions = $state<ManagedVersion[]>([]);
  let managedLoading = $state(false);
  let managedError = $state('');
  let managedRequestId = 0;
  let searchInput = $state<HTMLInputElement>();
  let systemDark = $state(false);
  let destroyed = false;
  let startupPageApplied = false;
  let githubVisited = $state(true);
  let visitedLocal = $state(new Set<Page>());
  $effect(() => {
    if (['library', 'updates', 'downloads'].includes(page) && !visitedLocal.has(page)) visitedLocal = new Set([...visitedLocal, page]);
  });
  // Keep the visited GitHub workspace in memory; it owns its loaded rows and scroll position.
  $effect(() => { if (page === 'github') githubVisited = true; });
  let refreshRequested = false;
  let refreshPromise: Promise<void> | null = null;
  let unlisten: (() => void) | undefined;
  let toastId = 0;
  const toastTimers = new Set<ReturnType<typeof setTimeout>>();
  const busy = $derived(pending.size > 0);
  const selectedProject = $derived(snapshot?.projects.find((project) => project.id === selectedId) ?? null);
  const managedProjectId = $derived(modal?.kind === 'managed' ? modal.projectId : null);
  const managedProject = $derived(snapshot?.projects.find((project) => project.id === managedProjectId) ?? null);
  const showingDetails = $derived(selectedProject !== null && ['home', 'library', 'updates'].includes(page));
  const pageLabels: Record<Page, string> = { home: '首页', library: '资源库', updates: '可更新', downloads: '下载', github: 'GitHub', agent: 'AI 连接', settings: '设置' };

  let addRepository = $state('');
  let addMode = $state<ManagementMode>('software');
  let addChannel = $state<Channel>('stable');
  let addPreference = $state<AssetPreference>('auto');
  let addVersion = $state('');
  let addPath = $state('');
  let addAdvanced = $state(false);
  let markVersion = $state('');
  let markPath = $state('');
  let markExecutable = $state('');
  let markConfirmed = $state(false);
  let noteName = $state('');
  let noteDescription = $state('');
  function noteFor(project: Project): RepositoryNote | null { return snapshot?.repositoryNotes.find((note) => note.repository.toLowerCase() === `${project.owner}/${project.repo}`.toLowerCase()) ?? null; }
  function projectLabel(project: Project) { return projectPresentation(project, noteFor(project)).label; }
  let importContents = $state('');
  const parsedRepository = $derived(parseRepository(addRepository));

  $effect(() => {
    const theme = snapshot?.settings.theme ?? 'system';
    const resolved = theme === 'system' ? systemDark ? 'dark' : 'light' : theme;
    document.documentElement.dataset.theme = resolved;
    document.documentElement.dataset.colorScheme = snapshot?.settings.colorScheme ?? 'blue';
    document.documentElement.dataset.material = snapshot?.settings.material ?? 'acrylic';
    document.documentElement.dataset.blur = String(snapshot?.settings.blur ?? true);
    document.documentElement.style.colorScheme = resolved;
    if (desktopAvailable()) void getCurrentWindow().setTheme(resolved).catch(() => {});
  });

  function notify(message: string, tone: Tone = 'success') {
    if (destroyed) return;
    const id = ++toastId;
    toasts = [...toasts.slice(-3), { id, message, tone }];
    const timer = setTimeout(() => { toasts = toasts.filter((toast) => toast.id !== id); toastTimers.delete(timer); }, tone === 'danger' ? 9_000 : 5_500);
    toastTimers.add(timer);
  }
  function setPending(key: string, value: boolean) { const next = new Set(pending); if (value) next.add(key); else next.delete(key); pending = next; }
  function previewNotice() { const message = '此功能请在 ReleaseHub 桌面版中使用。'; if (modal) formError = message; notify(message, 'warning'); }
  function reportError(error: unknown) { const message = readableError(error); operationError = message; if (modal) formError = message; notify(message, 'danger'); }

  async function refreshSnapshot() {
    if (preview || destroyed) return;
    refreshRequested = true;
    if (refreshPromise) return refreshPromise;
    refreshing = true;
    refreshPromise = (async () => {
      while (refreshRequested && !destroyed) {
        refreshRequested = false;
        const atRequestStart = snapshot;
        const next = await dispatch('snapshot', {});
        if (!destroyed && !preview) snapshot = mergeSnapshotDownloads(snapshot, next, atRequestStart);
      }
    })();
    try { await refreshPromise; } finally { refreshPromise = null; refreshing = false; }
  }

  async function operate<A extends Action>(action: A, payload: Actions[A]['payload'], options: { refresh?: boolean; success?: string; key?: string } = {}): Promise<Actions[A]['result'] | undefined> {
    if (preview) { previewNotice(); return undefined; }
    const key = options.key ?? action;
    if (pending.has(key)) return undefined;
    setPending(key, true); formError = ''; operationError = '';
    try {
      const result = await dispatch(action, payload);
      if (options.refresh !== false) {
        try { await refreshSnapshot(); } catch (error) { operationError = `操作已处理，但状态刷新失败：${readableError(error)}。请点击右上角刷新。`; }
      }
      if (options.success) notify(options.success);
      return result;
    } catch (error) { reportError(error); return undefined; }
    finally { setPending(key, false); }
  }

  async function initialize() {
    loading = true; startupError = '';
    if (!desktopAvailable()) {
      startupError = '请打开 ReleaseHub 桌面版使用，或进入界面预览。';
      loading = false; return;
    }
    try {
      await refreshSnapshot();
      if (snapshot && !startupPageApplied) { page = snapshot.settings.startupPage; startupPageApplied = true; }
      if (!unlisten && !destroyed) {
        try {
          const release = await watchChanges(
            () => { void refreshSnapshot().catch(reportError); },
            (record) => {
              if (destroyed || preview || !snapshot) return true;
              const next = applyDownloadProgress(snapshot, record);
              if (!next) return false;
              snapshot = next;
              return true;
            },
          );
          if (destroyed) release(); else unlisten = release;
        } catch { notify('实时状态通知暂不可用，仍可手动刷新；进行中的任务会定期同步。', 'warning'); }
      }
    } catch (error) { startupError = readableError(error); }
    finally { loading = false; }
  }

  onMount(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    systemDark = media.matches;
    const updateSystem = (event: MediaQueryListEvent) => { systemDark = event.matches; };
    media.addEventListener('change', updateSystem);
    void initialize();
    const poll = setInterval(() => {
      // Normal changes arrive as events; polling is only a recovery path if subscribing failed.
      if (!unlisten && !preview && snapshot && !document.hidden && (snapshot.runtime.agent.enabled || snapshot.downloads.some((item) => ['queued', 'downloading', 'installing'].includes(item.status)) || snapshot.buildTasks.some((item) => ['claimed', 'running'].includes(item.status)))) void refreshSnapshot().catch(reportError);
    }, 5_000);
    return () => { destroyed = true; unlisten?.(); clearInterval(poll); toastTimers.forEach(clearTimeout); media.removeEventListener('change', updateSystem); };
  });

  async function enterPreview() {
    if (!import.meta.env.DEV || desktopAvailable()) return;
    const { createPreviewSnapshot } = await import('./lib/preview');
    snapshot = createPreviewSnapshot(); preview = true; startupError = ''; operationError = '';
  }
  function exitPreview() { modal = null; selectedId = null; snapshot = null; preview = false; page = 'home'; query = ''; startupError = '界面预览已退出。真实操作请使用 ReleaseHub 桌面版。'; }
  const pageState = new Map<Page, { query: string; selectedId: string | null; scrollTop: number }>();
  async function navigate(next: Page) {
    if (next === page) return;
    const content = document.querySelector<HTMLElement>('#main-content');
    pageState.set(page, { query, selectedId, scrollTop: content?.scrollTop ?? 0 });
    const previous = pageState.get(next);
    page = next; query = previous?.query ?? ''; selectedId = previous?.selectedId ?? null;
    await tick();
    const restored = document.querySelector<HTMLElement>('#main-content');
    if (restored && page === next) restored.scrollTop = previous?.scrollTop ?? 0;
  }
  function chooseProject(project: Project) { detailTab = 'notes'; selectedId = project.id; detailRevision += 1; }
  function showProjectBackup(project: Project) { detailTab = 'manage'; selectedId = project.id; detailRevision += 1; }
  function openProject(id: string) { if (!snapshot?.projects.some((project) => project.id === id)) { notify('这个资源已取消订阅，下载记录仍然保留。', 'warning'); return; } page = 'library'; query = ''; selectedId = id; }
  function openDialog(value: DialogState) { formError = ''; modal = value; }
  function closeDialog() { if (busy) return; modal = null; formError = ''; importContents = ''; managedRequestId += 1; managedVersions = []; managedError = ''; managedLoading = false; }
  function showAdd() { addMode = 'software'; addRepository = ''; addChannel = 'stable'; addPreference = 'auto'; addVersion = ''; addPath = ''; addAdvanced = false; openDialog({ kind: 'add' }); }
  function showMark(project: Project, suggestion?: string) { if (project.managementMode === 'files') return; markVersion = suggestion ?? project.installedVersion ?? ''; markPath = project.installedPath ?? ''; markExecutable = project.executablePath ?? ''; markConfirmed = false; openDialog({ kind: 'mark', project }); }
  function markDownloaded(id: string, version: string) { const project = snapshot?.projects.find((item) => item.id === id); if (project) showMark(project, version); else notify('请先重新订阅这个软件，再记录本机版本。', 'warning'); }
  function showInstall(download: DownloadRecord) { if (download.installable === false || download.kind === 'other') return; openDialog({ kind: 'install', download }); }

  function showManagedVersions(projectId: string) {
    if (busy) return;
    const project = snapshot?.projects.find((item) => item.id === projectId);
    if (!project) { notify('这个资源已取消订阅，已准备的文件仍然保留。请先重新订阅。', 'warning'); return; }
    if (project.installedSource !== 'managed') { notify('当前没有由追新管理的安装记录。手动记录的外部软件不在这里切换。', 'warning'); return; }
    openDialog({ kind: 'managed', projectId });
    void loadManagedVersions();
  }
  async function loadManagedVersions() {
    if (modal?.kind !== 'managed' || busy) return;
    const projectId = modal.projectId;
    const requestId = ++managedRequestId;
    const current = () => !destroyed && requestId === managedRequestId && modal?.kind === 'managed' && modal.projectId === projectId;
    managedVersions = []; managedLoading = true; managedError = ''; formError = '';
    try {
      let versions: ManagedVersion[];
      if (preview && import.meta.env.DEV) {
        const { createPreviewManagedVersions } = await import('./lib/preview');
        versions = createPreviewManagedVersions(projectId);
      } else {
        versions = await dispatch('list_managed_versions', { projectId });
      }
      if (current()) managedVersions = versions;
    } catch (error) { if (current()) managedError = readableError(error); }
    finally { if (current()) managedLoading = false; }
  }
  async function activateManagedVersion(downloadId: string, confirmed: boolean): Promise<boolean> {
    if (preview) { previewNotice(); return false; }
    const project = managedProject;
    const version = managedVersions.find((item) => item.downloadId === downloadId);
    if (busy || managedLoading || managedError || !project || !canActivateManagedVersion(project, version, confirmed)) return false;
    const result = await operate('activate_managed_version', { projectId: project.id, downloadId, confirmed: true });
    if (!result) return false;
    if (snapshot) snapshot = { ...snapshot, projects: snapshot.projects.map((item) => item.id === result.id ? result : item) };
    await loadManagedVersions();
    notify(`当前版本记录已切换为 ${result.installedVersion}。`);
    return true;
  }
  async function openManagedVersion(downloadId: string) {
    if (busy || managedLoading || managedProject?.installedSource !== 'managed' || !managedVersions.some((item) => item.downloadId === downloadId)) return;
    await operate('open_managed_version', { projectId: managedProject.id, downloadId }, { refresh: false });
  }

  async function checkUpdates(ids?: string[]) {
    const result = await operate('check_updates', { ...(ids ? { projectIds: ids } : {}), allowAutoDownload: true });
    if (result) notify(`已检查 ${result.checked} 项资源，发现 ${result.updates} 项更新${result.errors ? `，${result.errors} 项未完成，请查看资源详情` : '。'}`, result.errors ? 'warning' : 'success');
  }
  async function downloadProject(project: Project, quiet = false): Promise<boolean> {
    const asset = project.release?.assets.find((item) => item.id === project.selectedAssetId && (item.downloadable ?? item.compatible));
    if (!asset) { chooseProject(project); if (!quiet) notify('请先在详情的“发行附件”中选择需要下载的文件。', 'warning'); return false; }
    if (snapshot?.settings.useExternalDownloader) { showExternalDownload([project]); return false; }
    const result = await operate('download_asset', { projectId: project.id, assetId: asset.id }, { refresh: !quiet, key: `download:${project.id}` });
    if (result && !quiet) notify(`${project.name} 已加入下载队列。`);
    return result !== undefined;
  }
  async function batchDownload(projects: Project[]) {
    if (preview) { previewNotice(); return; }
    if (snapshot?.settings.useExternalDownloader) { showExternalDownload(projects); return; }
    setPending('batch-download', true);
    let queued = 0;
    try { for (const project of projects) if (await downloadProject(project, true)) queued += 1; await refreshSnapshot(); notify(`已将 ${queued} 项加入下载队列${projects.length > queued ? `；另有 ${projects.length - queued} 项未下载，请检查安装包或错误提示` : '。'}`, queued === projects.length ? 'success' : 'warning'); }
    catch (error) { reportError(error); } finally { setPending('batch-download', false); }
  }
  function showExternalDownload(projects: Project[], retry?: DownloadRecord) {
    if (preview) { previewNotice(); return; }
    openDialog({ kind: 'external', projects, ...(retry ? { retry } : {}) });
  }
  async function submitExternalDownload() {
    if (modal?.kind !== 'external' || busy) return;
    const request = modal; let sent = 0;
    if (request.retry) {
      const result = await operate('retry_external_download', { id: request.retry.id, confirmed: true });
      if (result) { closeDialog(); notify('下载链接已重新发送，请在外部下载器查看。'); }
      return;
    }
    for (const project of request.projects) {
      const asset = project.release?.assets.find(item => item.id === project.selectedAssetId && (item.downloadable ?? item.compatible));
      if (!asset) continue;
      const result = await operate('send_external_download', { projectId: project.id, assetId: asset.id, confirmed: true });
      if (result) sent += 1; else return;
    }
    closeDialog(); notify(`已处理 ${sent} 项下载链接；请在下载页查看是否已交给外部下载器。`);
  }
  async function importExternalDownload(download: DownloadRecord) {
    const result = await operate('import_external_download', { id: download.id, confirmed: true });
    if (result) notify('文件已导入并完成校验，可在下载页确认安装。');
  }
  async function updateProject(id: string, changes: ProjectChanges) { const result = await operate('update_project', { id, changes }, { success: '订阅偏好已保存。' }); if (result && ('channel' in changes || 'preference' in changes || 'assetPattern' in changes || 'managementMode' in changes)) await checkUpdates([id]); }
  async function saveSettings(changes: Partial<Settings>): Promise<boolean> {
    if (preview && snapshot) { snapshot = { ...snapshot, settings: { ...snapshot.settings, ...changes } }; notify('预览设置已更新。', 'neutral'); return true; }
    return await operate('save_settings', { settings: changes }, { success: '设置已保存。' }) !== undefined;
  }
  async function saveProxy(proxy: ProxySettings, password?: string): Promise<boolean> {
    if (preview && snapshot) { snapshot = { ...snapshot, settings: { ...snapshot.settings, proxy } }; notify('预览设置已更新。', 'neutral'); return true; }
    return await operate('save_proxy', { proxy, ...(password === undefined ? {} : { password }) }, { success: '代理设置已保存，新请求将使用新设置。' }) !== undefined;
  }
  async function copyDiagnostic(projectId: string) {
    const result = await operate('get_agent_diagnostic', { projectId }, { refresh: false });
    if (result) await copy(JSON.stringify(result, null, 2));
  }
  async function getIntegration(): Promise<AgentIntegration | null> { return preview ? null : dispatch('get_agent_integration', {}); }
  async function addGithubRepository(repository: GithubRepository): Promise<boolean> {
    if (preview) { previewNotice(); return false; }
    if (repository.private) { notify('私有仓库暂不加入更新列表。', 'warning'); return false; }
    if (isSubscribed(repository.fullName, snapshot?.projects ?? [])) { notify('这个仓库已经订阅。', 'neutral'); return true; }
    const input: AddProjectInput = { repository: repository.fullName, channel: 'stable', preference: 'auto' };
    const project = await operate('add_project', { input }, { key: `github-add:${repository.fullName.toLowerCase()}`, success: '已加入资源库，可前往检查发行版。' });
    if (project && snapshot && !snapshot.projects.some((item) => item.id === project.id)) snapshot = { ...snapshot, projects: [...snapshot.projects, project] };
    return project !== undefined;
  }
  async function openUrl(url: string) { const safe = safeGithubUrl(url); if (!safe) { notify('只支持在系统浏览器打开安全的 HTTPS GitHub 链接，请到项目原文查看其他地址。', 'warning'); return; } await operate('open_url', { url: safe }, { refresh: false }); }
  async function copy(text: string) { try { await navigator.clipboard.writeText(text); notify('已复制到剪贴板。'); } catch { notify('剪贴板不可用，请选择文本后按 Ctrl+C 复制。', 'warning'); } }
  async function exportFile() { const result = await operate('export_library_file', {}, { refresh: false }); if (result) notify(`订阅与备注已导出到 ${result.path}。`); }
  async function importFile() { const result = await operate('import_library_file', {}); if (result) notify(`已导入 ${result.imported} 项订阅、${result.notesImported} 条备注，跳过 ${result.skipped} 项重复订阅。`); }
  async function copyExport() { const result = await operate('export_library', {}, { refresh: false }); if (result !== undefined) await copy(result); }
  async function pickPath(target: 'add' | 'mark') {
    const result = await operate('pick_directory', {}, { refresh: false });
    if (result) { if (target === 'add') addPath = result; else { if (markPath !== result) markExecutable = ''; markPath = result; markConfirmed = false; } }
  }
  async function pickMarkExecutable() {
    if (modal?.kind !== 'mark') return;
    const result = await operate('pick_executable', { projectId: modal.project.id }, { refresh: false });
    if (result) { markExecutable = result.executablePath; markPath = result.installedPath; markConfirmed = false; }
  }
  function showNote(project: Project) { const note = noteFor(project); noteName = note?.name ?? ''; noteDescription = note?.description ?? ''; openDialog({ kind: 'note', project }); }
  async function submitNote(event: SubmitEvent) {
    event.preventDefault(); if (modal?.kind !== 'note') return;
    const result = await operate('save_repository_note', { repository: `${modal.project.owner}/${modal.project.repo}`, name: noteName.trim(), description: noteDescription.trim() }, { success: noteName.trim() || noteDescription.trim() ? '备注已保存。' : '备注已清除，恢复显示原名称。' });
    if (result !== undefined) modal = null;
  }
  async function refreshView() { if (preview) { previewNotice(); return; } try { await refreshSnapshot(); operationError = ''; notify('已同步最新本机状态。'); } catch (error) { reportError(error); } }

  async function submitAdd(event: SubmitEvent) {
    event.preventDefault(); formError = '';
    const repository = parseRepository(addRepository);
    if (!repository) { formError = '请输入“作者/仓库”，或完整的 https://github.com/作者/仓库 地址。'; return; }
    const input: AddProjectInput = { repository, managementMode: addMode, channel: addChannel, preference: addPreference, ...(addMode === 'software' && addVersion.trim() ? { installedVersion: addVersion.trim() } : {}), ...(addMode === 'software' && addPath.trim() ? { installedPath: addPath.trim() } : {}) };
    const project = await operate('add_project', { input }, { success: '已加入资源库。' });
    if (project) { modal = null; page = 'library'; query = ''; selectedId = project.id; await checkUpdates([project.id]); }
  }
  async function submitMark(event: SubmitEvent) {
    event.preventDefault(); if (modal?.kind !== 'mark' || !markConfirmed || !markVersion.trim()) return;
    const result = await operate('mark_installed', { projectId: modal.project.id, version: markVersion.trim(), ...(markPath.trim() ? { path: markPath.trim() } : {}), ...(markExecutable.trim() ? { executablePath: markExecutable.trim() } : {}) }, { success: '已按你的确认记录本机版本。' });
    if (result) modal = null;
  }
  async function submitInstall(plan: InstallPreview, archiveRoot?: string) {
    if (modal?.kind !== 'install' || plan.downloadId !== modal.download.id) return;
    const result = await operate('install_download', { id: plan.downloadId, confirmed: true, targetPath: plan.targetPath, previousVersion: plan.previousVersion, ...(archiveRoot ? { archiveRoot } : {}) });
    if (result) { modal = null; notify(result.message || (result.installed ? '已备份并更新到软件原目录。' : '已创建文件备份并打开官方安装向导，完成后请手动记录版本。'), result.installed ? 'success' : 'neutral'); }
  }
  async function runProject(project: Project) {
    if (project.managementMode === 'files') { showProjectFiles(project); return; }
    if (preview) { previewNotice(); return; }
    if (!project.executablePath) {
      const saved = await operate('pick_project_executable', { projectId: project.id });
      if (saved) { chooseProject(saved); notify('已保存启动程序并自动记录文件夹。点击“打开软件”运行。'); }
      return;
    }
    await operate('open_project', { projectId: project.id }, { refresh: false });
  }
  async function chooseExecutable(project: Project) { const saved = await operate('pick_project_executable', { projectId: project.id }); if (saved) notify('已保存启动程序并自动记录文件夹。'); }
  function showProjectFiles(project: Project) { void navigate('downloads').then(() => { query = project.name; }); }
  async function deleteDownload() {
    if (modal?.kind !== 'deleteDownload') return;
    const result = await operate('delete_download', { id: modal.download.id, confirmed: true }, { success: '安装包和这条下载记录已删除，已安装软件与备份保留。' });
    if (result !== undefined) modal = null;
  }
  async function applyBackup() {
    if (modal?.kind !== 'backup') return;
    const action = modal.operation === 'restore' ? 'restore_backup' : 'delete_backup';
    const result = await operate(action, { projectId: modal.backup.projectId, confirmed: true }, { success: action === 'restore_backup' ? '已恢复更新前文件与版本记录。' : '备份已删除，这个回退点已失效。' });
    if (result !== undefined) { modal = null; detailRevision += 1; }
  }
  async function submitImport(event: SubmitEvent) {
    event.preventDefault(); if (importContents.length > 1_000_000) { formError = '清单过大，请通过“导入订阅”选择文件。'; return; }
    const result = await operate('import_library', { contents: importContents });
    if (result) { importContents = ''; modal = null; notify(`已导入 ${result.imported} 项订阅、${result.notesImported} 条备注，跳过 ${result.skipped} 项重复订阅。`); }
  }
  async function removeProject(project: Project) { const result = await operate('remove_project', { id: project.id }, { success: '已取消订阅，下载文件和已安装软件保持不变。' }); if (result !== undefined) { modal = null; if (selectedId === project.id) selectedId = null; } }
  async function hotkeys(event: KeyboardEvent) { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k' && snapshot && !modal && !document.querySelector('dialog[open]')) { event.preventDefault(); const githubSearch = page === 'github' ? document.querySelector<HTMLInputElement>('#github-search') : null; if (githubSearch) { githubSearch.focus(); return; } if (!['library', 'updates', 'downloads'].includes(page)) navigate('library'); await tick(); searchInput?.focus(); } if (event.key === 'Escape' && !modal) selectedId = null; }
</script>

<svelte:window onkeydown={hotkeys} />
<svelte:head><title>ReleaseHub 追新 · 发行资源工作空间</title><meta name="description" content="集中管理 GitHub 软件更新、Stars 分类、项目备注与首页动态。" /></svelte:head>

{#if !snapshot}
  <div class="startup-titlebar"><TitleBar title="ReleaseHub · 追新" searchable={false} onerror={reportError} /></div>
  <main class="startup-shell"><div class="startup-brand"><BrandMark size={40} /><strong>ReleaseHub <span>追新</span></strong></div><section class="startup-card" aria-live="polite">{#if loading}<span class="startup-icon"><Icon name="loader" class="spinning" size={30} /></span><h1>正在打开你的工作空间</h1><p>读取资源库与本机设置，请稍候。</p><div class="skeleton-lines"><span></span><span></span><span></span></div>{:else}<span class="startup-icon"><Icon name="disconnected" size={29} /></span><h1>请在桌面程序中继续</h1><p class="startup-error">{startupError || '桌面服务未连接，请重新打开应用。'}</p><button class="button secondary" onclick={() => void initialize()}><Icon name="refresh" size={16} />重新连接桌面服务</button>{#if import.meta.env.DEV && !desktopAvailable()}<div class="preview-gate"><span class="status-badge warning">仅限开发环境</span><h2>只是想看看界面？</h2><p>使用示例资源与账号查看各页面。</p><button class="button primary" onclick={() => void enterPreview()}>进入界面预览<Icon name="arrowRight" size={16} /></button></div>{/if}{/if}</section><p class="startup-footer">来自 GitHub 的变化，在这里从容掌握。</p></main>
{:else}
  <div class="app-shell"><Sidebar {page} {snapshot} {preview} {busy} onnavigate={navigate} onadd={showAdd} ondata={() => void operate('open_data_directory', {}, { refresh: false })} onreorder={(navigationOrder: WorkspacePage[]) => saveSettings({ navigationOrder })} />
    <div class="workspace"><TitleBar title={pageLabels[page]} bind:query bind:inputElement={searchInput} {busy} {refreshing} onsearch={() => { if (!['library', 'updates', 'downloads'].includes(page)) page = 'library'; }} onrefresh={() => void refreshView()} onerror={reportError} />
      {#if preview}<div class="preview-banner"><Icon name="monitor" size={15} /><strong>界面预览</strong><span>示例版本、账号与状态</span><button onclick={exitPreview}>退出预览<Icon name="close" size={13} /></button></div>{/if}
      {#if operationError}<div class="operation-error" role="alert"><Icon name="alert" size={17} /><span>{operationError}</span><button class="icon-button" aria-label="收起错误提示" onclick={() => operationError = ''}><Icon name="close" size={16} /></button></div>{/if}
      <div class="workspace-content" class:with-details={showingDetails}><main class="page-content" id={!['github', 'library', 'updates', 'downloads'].includes(page) ? 'main-content' : undefined} hidden={['github', 'library', 'updates', 'downloads'].includes(page)}>
        {#if page === 'home'}<Home {snapshot} {busy} {selectedId} {preview} active={page === 'home'} oninstall={showInstall} onopen={(url) => void openUrl(url)} onGithubAdd={addGithubRepository} onnotice={notify} onGithubLogin={() => void navigate('github')} onpage={navigate} onselect={chooseProject} onadd={showAdd} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} />
        {:else if page === 'agent'}<Connections settings={snapshot.settings} runtime={snapshot.runtime} {busy} {preview} onsave={(changes) => void saveSettings(changes)} onintegration={getIntegration} oncopy={(text) => void copy(text)} />
        {:else if page === 'settings'}<SettingsPage settings={snapshot.settings} runtime={snapshot.runtime} {busy} {preview} onsave={saveSettings} onproxy={saveProxy} onproxytest={(proxy, password) => dispatch('test_proxy', { proxy, ...(password === undefined ? {} : { password }) })} onintegration={getIntegration} ongithub={() => navigate('github')} ondata={() => void operate('open_data_directory', {}, { refresh: false })} onexport={() => void exportFile()} onimport={() => void importFile()} ontextimport={() => { importContents = ''; openDialog({ kind: 'import' }); }} oncopyexport={() => void copyExport()} oncopy={(text) => void copy(text)} onopen={(url) => void openUrl(url)} />{/if}
      </main>
      {#each ['library', 'updates'] as localPage}
         {#if visitedLocal.has(localPage as Page) || page === localPage}<main class="page-content" id={page === localPage ? 'main-content' : undefined} hidden={page !== localPage}><Library projects={snapshot.projects} notes={snapshot.repositoryNotes} layout={snapshot.settings.libraryLayout ?? 'three'} onlayout={(libraryLayout) => void saveSettings({ libraryLayout })} onnote={showNote} updatesOnly={localPage === 'updates'} active={page === localPage} {query} {selectedId} {busy} onadd={showAdd} onselect={chooseProject} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} onbatchdownload={(projects) => void batchDownload(projects)} onOpen={(project) => void runProject(project)} onFiles={showProjectFiles} onBackup={showProjectBackup} onRepository={(project) => void openUrl(`https://github.com/${project.owner}/${project.repo}`)} /></main>{/if}
       {/each}
       {#if visitedLocal.has('downloads') || page === 'downloads'}<main class="page-content" id={page === 'downloads' ? 'main-content' : undefined} hidden={page !== 'downloads'}><Downloads downloads={snapshot.downloads} {query} {busy} onexternalretry={(download) => showExternalDownload([], download)} onexternalimport={(download) => void importExternalDownload(download)} onretry={(id) => void operate('retry_download', { id }, { success: '已重新加入下载队列。' })} oncancel={(id) => void operate('cancel_download', { id }, { success: '已请求取消下载。' })} oninstall={showInstall} onDelete={(download) => openDialog({ kind: 'deleteDownload', download })} onfolder={(id) => void operate('open_download', { id }, { refresh: false })} onmark={markDownloaded} onproject={openProject} oncopy={(text) => void copy(text)} onversions={showManagedVersions} /></main>{/if}
       {#if githubVisited || page === 'github'}<main class="page-content github-workspace" id={page === 'github' ? 'main-content' : undefined} hidden={page !== 'github'}><Github projects={snapshot.projects} repositoryNotes={snapshot.repositoryNotes} {preview} preload active={page === 'github'} initialTab={snapshot.settings.startupGithubTab} onadd={addGithubRepository} onopen={(url) => void openUrl(url)} oncopy={(text) => void copy(text)} onnotice={notify} onnotes={(notes) => { if (snapshot) snapshot = { ...snapshot, repositoryNotes: notes }; }} /></main>{/if}
      {#if showingDetails && selectedProject}<button class="detail-backdrop" aria-label="关闭资源详情" onclick={() => selectedId = null}></button>{#key `${selectedProject.id}:${detailRevision}`}<ProjectDetail project={selectedProject} note={noteFor(selectedProject)} onnote={showNote} initialTab={detailTab} {busy} {preview} onexternal={(project) => showExternalDownload([project])} onrun={(project) => void runProject(project)} onexecutable={(project) => void chooseExecutable(project)} onrestore={(backup) => openDialog({ kind: 'backup', operation: 'restore', backup })} ondeletebackup={(backup) => openDialog({ kind: 'backup', operation: 'delete', backup })} onclose={() => selectedId = null} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} onupdate={(id, changes) => void updateProject(id, changes)} onmark={showMark} onversions={showManagedVersions} ondiagnostic={(id) => void copyDiagnostic(id)} onremove={(project) => openDialog({ kind: 'remove', project })} onopen={(url) => void openUrl(url)} />{/key}{/if}</div>
    </div>
  </div>
{/if}

{#if modal?.kind === 'add'}
  <Modal title="添加资源" description="订阅 GitHub 项目，管理软件更新或发行附件。" onclose={closeDialog}>
    <form class="modal-form" onsubmit={submitAdd}>
      <label for="new-repository">GitHub 项目地址 <span class="required-mark">*</span></label>
      <div class="input-leading"><Icon name="github" size={18} /><input id="new-repository" bind:value={addRepository} data-initial-focus placeholder="作者/仓库，或粘贴 GitHub 项目链接" required maxlength="500" spellcheck="false" /></div>
      {#if addRepository}<p class={parsedRepository ? 'field-hint success-text' : 'field-hint'}>{parsedRepository ? `将订阅 ${parsedRepository}` : '例如 Lake1059/FFmpegFreeUI；仅支持 GitHub 项目地址'}</p>{/if}
      <fieldset class="resource-mode"><legend>怎样管理这个项目？</legend>
        <label class:chosen={addMode === 'software'}><input type="radio" name="management-mode" bind:group={addMode} value="software" /><span><strong>软件</strong><small>推荐 Windows 安装包，记录本机版本</small></span></label>
        <label class:chosen={addMode === 'files'}><input type="radio" name="management-mode" bind:group={addMode} value="files" /><span><strong>文件</strong><small>下载文档、模型、模组等发行附件</small></span></label>
      </fieldset>
      <div class="form-columns"><label>订阅渠道<select bind:value={addChannel}><option value="stable">正式版 · 推荐</option><option value="preview">包含预览版</option></select></label>
        {#if addMode === 'software'}<label>安装包偏好<select bind:value={addPreference}><option value="auto">自动推荐</option><option value="portable">优先免安装包</option><option value="installer">优先安装程序</option></select></label>{/if}
      </div>
      {#if addMode === 'software'}
        <button type="button" class="text-button advanced-toggle" aria-expanded={addAdvanced} onclick={() => addAdvanced = !addAdvanced}><Icon name="chevronDown" size={15} />{addAdvanced ? '收起本机版本记录' : '已经安装过？记录本机版本（可选）'}</button>
        {#if addAdvanced}<div class="advanced-fields"><label>已安装版本<input bind:value={addVersion} placeholder="例如 v1.2.3，不确定就留空" maxlength="120" /></label><label>安装位置<div class="input-with-button"><input bind:value={addPath} placeholder="选择已安装软件的文件夹" maxlength="1000" /><button type="button" class="button secondary" disabled={busy} onclick={() => void pickPath('add')}>浏览</button></div></label><p class="field-hint">记录后可对比本机版本与最新发行版。</p></div>{/if}
      {:else}<p class="field-hint">从 Releases 中逐个选择附件，单个文件最多 4GB。文件下载后不会自动运行或安装。</p>{/if}
      {#if formError}<p class="field-error" role="alert">{formError}</p>{/if}
      <div class="modal-footer"><span class="field-hint"><Icon name="github" size={14} />公开项目无需登录</span><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !parsedRepository}><Icon name={busy ? 'loader' : 'plus'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在添加' : '添加资源'}</button></div>
    </form>
  </Modal>
{:else if modal?.kind === 'external'}
  <Modal title={modal.retry ? '重新发送下载链接？' : '交给外部下载器？'} description={snapshot?.settings.externalDownloader?.name ?? 'AB Download Manager'} onclose={closeDialog}><div class="modal-form"><p class="confirmation-details">{modal.retry ? '请先到外部下载器检查原任务，确认没有重复任务后再发送。' : `将 ${modal.projects.length} 项公开发行链接发送到本机下载器。下载完成后，在追新的下载页导入文件并校验；安装仍需另行确认。`}</p><p class="field-hint">{snapshot?.settings.externalDownloader?.endpoint ?? 'http://127.0.0.1:15151/add'}</p>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" disabled={busy} onclick={() => void submitExternalDownload()}>{busy ? '正在发送' : modal.retry ? '确认重新发送' : '确认发送链接'}</button></div></div></Modal>
{:else if modal?.kind === 'install'}
  <InstallConfirmation download={modal.download} {preview} {busy} onclose={closeDialog} onconfirm={submitInstall} />
{:else if modal?.kind === 'deleteDownload'}
  <Modal title="删除这个已下载安装包？" description={modal.download.fileName} onclose={closeDialog}><div class="modal-form"><p class="confirmation-details">删除这条下载记录及其受管文件，预计释放 {formatBytes(modal.download.totalBytes)}。已安装软件和最近一次备份保留，需要时可从资源库重新下载。</p><div class="modal-footer"><button class="button secondary" disabled={busy} onclick={closeDialog}>保留安装包</button><button class="button danger" disabled={busy} onclick={() => void deleteDownload()}>确认删除安装包</button></div></div></Modal>
{:else if modal?.kind === 'backup'}
  <Modal title={modal.operation === 'restore' ? '恢复更新前的文件状态？' : '删除最近一次备份？'} description={modal.backup.version ?? '更新前文件备份'} onclose={closeDialog}><div class="modal-form"><p class="confirmation-details">{modal.operation === 'restore' ? '恢复会移除更新后新增的文件，覆盖现有配置，并同步更新前版本记录。请先关闭这个软件；此后的配置改动也会被撤回。' : '仅删除这份备份，不删除当前软件或安装包。删除后这个回退点失效，操作无法撤回。'}</p><p class="field-hint">{modal.backup.targetPath}</p>{#if modal.backup.kind === 'installer'}<p class="warning-text">此操作只处理文件，不能完整回退 EXE / MSI 修改的注册表、服务等。</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class={modal.operation === 'delete' ? 'button danger' : 'button primary'} disabled={busy} onclick={() => void applyBackup()}>{modal.operation === 'restore' ? '确认恢复文件' : '确认删除备份'}</button></div></div></Modal>
{:else if modal?.kind === 'managed'}
  {#key modal.projectId}<ManagedVersions project={managedProject} versions={managedVersions} loading={managedLoading} error={managedError} actionError={formError} {busy} {preview} onclose={closeDialog} onrefresh={() => void loadManagedVersions()} onopen={(id) => void openManagedVersion(id)} onactivate={activateManagedVersion} />{/key}
{:else if modal?.kind === 'mark'}
  <Modal title="记录已安装版本" description={`填写 ${projectLabel(modal.project)} 在本机的版本，选择启动程序会自动记录文件夹。`} onclose={closeDialog}>
    <form class="modal-form" onsubmit={submitMark}>
      <label for="mark-version">实际已安装版本<input id="mark-version" bind:value={markVersion} required maxlength="120" placeholder="例如 v1.2.3" data-initial-focus oninput={() => markConfirmed = false} /></label>
      <label for="mark-executable">启动程序 <span class="optional">可选</span><div class="input-with-button"><input id="mark-executable" value={markExecutable} readonly placeholder="选择软件的 EXE 文件" /><button type="button" class="button secondary" disabled={busy} onclick={() => void pickMarkExecutable()}>选择启动程序</button></div></label>
      <label for="mark-path">安装位置 <span class="optional">自动记录，也可只选目录</span><div class="input-with-button"><input id="mark-path" bind:value={markPath} placeholder="选好启动程序后自动填入" maxlength="4096" oninput={() => { markExecutable = ''; markConfirmed = false; }} /><button type="button" class="button secondary" disabled={busy} onclick={() => void pickPath('mark')}>只选目录</button></div></label>
      <label class="confirmation-check"><input type="checkbox" bind:checked={markConfirmed} required />我已确认此版本在本机完成安装。</label>
      {#if formError}<p class="field-error" role="alert">{formError}</p>{/if}
      <div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !markConfirmed || !markVersion.trim()}>确认记录</button></div>
    </form>
  </Modal>
{:else if modal?.kind === 'note'}
  <Modal title="编辑备注" description="标题和描述都可留空。有备注时只显示备注，不显示原名称。" onclose={closeDialog}>
    <form class="modal-form" onsubmit={submitNote}>
      <label for="note-name">备注标题 <span class="optional">可选</span><input id="note-name" bind:value={noteName} maxlength="80" placeholder="写一个好认的名字" data-initial-focus /></label>
      <label for="note-description">备注描述 <span class="optional">可选</span><input id="note-description" bind:value={noteDescription} maxlength="240" placeholder="用途或一句简短说明" /></label>
      <p class="field-hint">只填描述也可以；描述会用较小字号显示。两项都留空并保存，即可恢复原名称。</p>
      {#if formError}<p class="field-error" role="alert">{formError}</p>{/if}
      <div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy}>保存备注</button></div>
    </form>
  </Modal>
{:else if modal?.kind === 'remove'}
  <Modal title="取消订阅这个资源？" description={projectLabel(modal.project)} onclose={closeDialog}><div class="modal-form"><p class="confirmation-details">从资源库移除这条订阅，保留已有的下载记录与文件。</p>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={busy} onclick={closeDialog}>继续订阅</button><button class="button danger" disabled={busy} onclick={() => { if (modal?.kind === 'remove') void removeProject(modal.project); }}>仅取消订阅</button></div></div></Modal>
{:else if modal?.kind === 'import'}
  <Modal title="粘贴订阅清单" description="导入订阅、备注和偏好，跳过重复订阅。" wide onclose={closeDialog}><form class="modal-form" onsubmit={submitImport}><label>订阅清单<textarea bind:value={importContents} rows="10" class="mono" placeholder="在这里粘贴 ReleaseHub 导出的完整 JSON" required maxlength="1000000" spellcheck="false" data-initial-focus></textarea></label><p class="field-hint">支持 ReleaseHub 导出的 JSON 清单，最多 1 MB。</p>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !importContents.trim()}><Icon name="import" size={16} />导入订阅</button></div></form></Modal>
{/if}

<div class="toast-stack" aria-live="polite" aria-atomic="false">{#each toasts as toast (toast.id)}<div class={`toast ${toast.tone}`} role={toast.tone === 'danger' ? 'alert' : 'status'}><Icon name={toast.tone === 'danger' ? 'alert' : toast.tone === 'warning' ? 'info' : toast.tone === 'success' ? 'success' : 'info'} size={19} /><p>{toast.message}</p><button class="icon-button" aria-label="关闭通知" onclick={() => toasts = toasts.filter((item) => item.id !== toast.id)}><Icon name="close" size={14} /></button></div>{/each}</div>
