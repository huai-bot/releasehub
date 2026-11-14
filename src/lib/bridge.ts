import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

export async function watchChanges(onChange: () => void): Promise<UnlistenFn> {
  const releases: UnlistenFn[] = [];
  try {
    releases.push(await listen('releasehub:changed', onChange));
    releases.push(await listen('releasehub:download', onChange));
    return () => releases.forEach((release) => release());
  } catch (error) {
    releases.forEach((release) => release());
    throw error;
  }
}
