import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isDownloadRecord } from './download-state';
import type {
  AddProjectInput, AgentConnection, BuildTask, CheckResult, DownloadRecord,
  InstallResult, ManagedVersion, Project, ProjectChanges, Settings, Snapshot,
} from './types';

/** Keep every desktop operation behind the one audited host command. */
export interface Actions {
  snapshot: { payload: Record<string, never>; result: Snapshot };
  add_project: { payload: { input: AddProjectInput }; result: Project };
  update_project: { payload: { id: string; changes: ProjectChanges }; result: Project };
  remove_project: { payload: { id: string }; result: null };
  check_updates: { payload: { projectIds?: string[]; allowAutoDownload?: boolean }; result: CheckResult };
  download_asset: { payload: { projectId: string; assetId: number }; result: DownloadRecord };
  retry_download: { payload: { id: string }; result: DownloadRecord };
  cancel_download: { payload: { id: string }; result: null };
  install_download: { payload: { id: string; confirmed: true }; result: InstallResult };
  mark_installed: { payload: { projectId: string; version: string; path?: string }; result: Project };
  list_managed_versions: { payload: { projectId: string }; result: ManagedVersion[] };
  activate_managed_version: { payload: { projectId: string; downloadId: string; confirmed: true }; result: Project };
  save_settings: { payload: { settings: Partial<Settings> }; result: Settings };
  export_library: { payload: Record<string, never>; result: string };
  import_library: { payload: { contents: string }; result: { imported: number; skipped: number } };
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
  import_library_file: { payload: Record<string, never>; result: { imported: number; skipped: number } | null };
  open_url: { payload: { url: string }; result: null };
  open_download: { payload: { id: string }; result: null };
  open_data_directory: { payload: Record<string, never>; result: null };
  open_managed_version: { payload: { projectId: string; downloadId: string }; result: null };
  set_github_token: { payload: { token: string }; result: null };
  reveal_agent_connection: { payload: Record<string, never>; result: AgentConnection };
  rotate_agent_token: { payload: Record<string, never>; result: AgentConnection };
}

export type Action = keyof Actions;
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
