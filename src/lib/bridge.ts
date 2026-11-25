import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isDownloadRecord } from './download-state';
import type {
  AddProjectInput, AgentConnection, AgentIntegration, BuildTask, CheckResult, DownloadRecord,
  GithubCacheEntry, GithubCacheOperation, GithubCacheStats, GithubActivityPage, GithubFeedPage, GithubListRepositoryPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus, GithubUserList,
  BackupRecord, InstallationCheck, InstallPreview, InstallResult, ManagedVersion, Project, ProjectChanges, ProxySettings, ProxyTestResult, RepositoryNote, Settings, Snapshot,
} from './types';

/** Keep every desktop operation behind the one audited host command. */
export interface Actions {
  snapshot: { payload: Record<string, never>; result: Snapshot };
  add_project: { payload: { input: AddProjectInput }; result: Project };
  update_project: { payload: { id: string; changes: ProjectChanges }; result: Project };
  remove_project: { payload: { id: string }; result: null };
  check_updates: { payload: { projectIds?: string[]; allowAutoDownload?: boolean }; result: CheckResult };
  mark_updates_read: { payload: { versions: { projectId: string; tag: string }[] }; result: number };
  download_asset: { payload: { projectId: string; assetId: number }; result: DownloadRecord };
  send_external_download: { payload: { projectId: string; assetId: number; confirmed: true }; result: DownloadRecord };
  retry_external_download: { payload: { id: string; confirmed: true }; result: DownloadRecord };
  import_external_download: { payload: { id: string; confirmed: true }; result: DownloadRecord | null };
  test_external_downloader: { payload: { config: import('./types').ExternalDownloader }; result: { ok: boolean; message: string } };
  retry_download: { payload: { id: string }; result: DownloadRecord };
  cancel_download: { payload: { id: string }; result: null };
  preview_install: { payload: { id: string; targetPath?: string }; result: InstallPreview };
  install_download: { payload: { id: string; confirmed: true; targetPath: string; archiveRoot?: string; previousVersion?: string | null }; result: InstallResult };
  verify_installations: { payload: { projectId?: string }; result: InstallationCheck[] };
  confirm_installation: { payload: { id: string }; result: Project };
  get_backup: { payload: { projectId: string }; result: BackupRecord | null };
  restore_backup: { payload: { projectId: string; confirmed: true }; result: Project };
  delete_backup: { payload: { projectId: string; confirmed: true }; result: null };
  delete_download: { payload: { id: string; confirmed: true }; result: null };
  pick_project_executable: { payload: { projectId: string }; result: Project | null };
  pick_executable: { payload: { projectId: string }; result: { executablePath: string; installedPath: string } | null };
  open_project: { payload: { projectId: string }; result: null };
  mark_installed: { payload: { projectId: string; version: string; path?: string; executablePath?: string }; result: Project };
  list_managed_versions: { payload: { projectId: string }; result: ManagedVersion[] };
  activate_managed_version: { payload: { projectId: string; downloadId: string; confirmed: true }; result: Project };
  save_settings: { payload: { settings: Partial<Settings> }; result: Settings };
  export_library: { payload: Record<string, never>; result: string };
  import_library: { payload: { contents: string }; result: { imported: number; skipped: number; notesImported: number } };
  create_build_task: { payload: { projectId: string; sourceRef?: string; instructions?: string }; result: BuildTask };
  cancel_build_task: { payload: { id: string }; result: BuildTask };
  list_build_tasks: { payload: Record<string, never>; result: BuildTask[] };
  claim_build_task: { payload: { id: string; agentName: string }; result: BuildTask };
  update_build_task: {
    payload: { id: string; status: 'running' | 'succeeded' | 'failed'; progress?: string; artifactUrl?: string; artifactSha256?: string; sourceCommit?: string; error?: string };
    result: BuildTask;
  };
  pick_directory: { payload: Record<string, never>; result: string | null };
  export_library_file: { payload: Record<string, never>; result: { path: string } | null };
  import_library_file: { payload: Record<string, never>; result: { imported: number; skipped: number; notesImported: number } | null };
  open_url: { payload: { url: string }; result: null };
  reader_open: { payload: { id: string; url: string; theme: Settings['theme']; bounds: ReaderBounds }; result: null };
  reader_resize: { payload: { id: string; bounds: ReaderBounds }; result: null };
  reader_close: { payload: { id: string }; result: null };
  reader_external: { payload: { id: string }; result: null };
  open_download: { payload: { id: string }; result: null };
  open_data_directory: { payload: Record<string, never>; result: null };
  open_managed_version: { payload: { projectId: string; downloadId: string }; result: null };
  get_agent_integration: { payload: Record<string, never>; result: AgentIntegration };
  get_agent_diagnostic: { payload: { projectId: string }; result: Record<string, unknown> };
  save_proxy: { payload: { proxy: ProxySettings; password?: string }; result: Settings };
  test_proxy: { payload: { proxy?: ProxySettings; password?: string }; result: ProxyTestResult };
  get_repository_notes: { payload: Record<string, never>; result: RepositoryNote[] };
  save_repository_note: { payload: { repository: string; name: string; description: string }; result: RepositoryNote | null };
  github_cache_read: { payload: { operation: GithubCacheOperation; parameters: Record<string, unknown> }; result: GithubCacheEntry | null };
  github_cache_stats: { payload: Record<string, never>; result: GithubCacheStats };
  github_cache_clear: { payload: Record<string, never>; result: null };
  github_status: { payload: Record<string, never>; result: GithubStatus };
  github_login: { payload: { lists?: boolean; privateRepositories?: boolean }; result: GithubStatus };
  github_cancel_login: { payload: Record<string, never>; result: GithubStatus };
  github_logout: { payload: Record<string, never>; result: GithubStatus };
  github_repositories: { payload: { tab: GithubRepositoryTab; query: string; page: number }; result: GithubRepositoryPage };
  github_repository: { payload: { repository: string }; result: GithubRepositoryDetail };
  github_set_star: { payload: { repository: string; starred: boolean; confirmed: true }; result: { starred: boolean } };
  github_update_repository: { payload: { repository: string; description: string; homepage: string; confirmed: true }; result: GithubRepository };
  github_activity: { payload: { page: number }; result: GithubActivityPage };
  github_lists: { payload: Record<string, never>; result: GithubUserList[] };
  github_save_list: { payload: { id?: string; name: string; description: string; confirmed: true }; result: GithubUserList };
  github_delete_list: { payload: { id: string; confirmed: true }; result: null };
  github_list_repositories: { payload: { listId: string; cursor?: string; query?: string }; result: GithubListRepositoryPage };
  github_repository_lists: { payload: { repository: string }; result: { listIds: string[] } };
  github_assign_lists: { payload: { repositories: string[]; listIds: string[]; mode: 'add' | 'remove' | 'replace'; confirmed: true }; result: { updated: number } };
  feed_status: { payload: Record<string, never>; result: GithubFeedPage };
  feed_connect: { payload: Record<string, never>; result: GithubFeedPage };
  feed_refresh: { payload: Record<string, never>; result: GithubFeedPage };
  feed_more: { payload: Record<string, never>; result: GithubFeedPage };
  feed_disconnect: { payload: Record<string, never>; result: GithubFeedPage };
  set_github_token: { payload: { token: string }; result: null };
  reveal_agent_connection: { payload: Record<string, never>; result: AgentConnection };
  rotate_agent_token: { payload: Record<string, never>; result: AgentConnection };
}

export type Action = keyof Actions;
export type ReaderBounds = { x: number; y: number; width: number; height: number };
export type ReaderEvent = { id: string; phase: 'loading' | 'ready' | 'title' | 'blocked' | 'download' | 'closed'; value: string };
export function watchReader(callback: (event: ReaderEvent) => void) {
  return listen<ReaderEvent>('releasehub:reader', event => callback(event.payload), { target: { kind: 'Webview', label: 'main' } });
}
export const desktopAvailable = (): boolean => isTauri();

export async function dispatch<A extends Action>(action: A, payload: Actions[A]['payload']): Promise<Actions[A]['result']> {
  if (!desktopAvailable()) {
    throw new Error('桌面服务未连接。请从 ReleaseHub 桌面程序打开；浏览器不能检查更新、下载或安装软件。');
  }
  return invoke<Actions[A]['result']>('dispatch', { action, payload });
}

/** onDownload returns true for an applied/old event, false when its identity needs a full refresh. */
export async function watchChanges(onChange: () => void, onDownload: (record: DownloadRecord) => boolean = () => false): Promise<UnlistenFn> {
  const releases: UnlistenFn[] = [];
  let active = true;
  let changeTimer: ReturnType<typeof setTimeout> | undefined;
  const scheduleChange = () => {
    if (!active || changeTimer !== undefined) return;
    changeTimer = setTimeout(() => { changeTimer = undefined; if (active) onChange(); }, 250);
  };
  const stop = () => {
    if (!active) return;
    active = false;
    if (changeTimer !== undefined) clearTimeout(changeTimer);
    // A closing webview must not prevent the remaining listeners from being released.
    releases.splice(0).forEach((release) => { try { release(); } catch { /* Already closed. */ } });
  };
  try {
    releases.push(await listen('releasehub:changed', scheduleChange));
    releases.push(await listen<unknown>('releasehub:download', (event) => {
      if (!active) return;
      if (isDownloadRecord(event.payload) && onDownload(event.payload)) return;
      scheduleChange();
    }));
    return stop;
  } catch (error) {
    stop();
    throw error;
  }
}
