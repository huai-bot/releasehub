export type Channel = 'stable' | 'preview';
export type AssetPreference = 'auto' | 'portable' | 'installer';
export type Theme = 'system' | 'light' | 'dark';
export type ProjectStatus = 'notChecked' | 'checking' | 'upToDate' | 'updateAvailable' | 'versionUnknown' | 'noRelease' | 'noCompatibleAsset' | 'error';
export type AssetKind = 'installer' | 'portable' | 'other';

export interface ReleaseAsset {
  id: number;
  name: string;
  size: number;
  url: string;
  contentType: string;
  digest: string | null;
  kind: AssetKind;
  compatible: boolean;
  score: number;
  reason: string;
}

export interface Release {
  id: number;
  tag: string;
  name: string;
  body: string;
  publishedAt: string;
  url: string;
  prerelease: boolean;
  assets: ReleaseAsset[];
}

export interface Project {
  id: string;
  owner: string;
  repo: string;
  name: string;
  description: string;
  url: string;
  avatarUrl: string | null;
  channel: Channel;
  preference: AssetPreference;
  assetPattern: string | null;
  installedVersion: string | null;
  installedPath: string | null;
  installedSource: 'manual' | 'managed' | null;
  release: Release | null;
  selectedAssetId: number | null;
  status: ProjectStatus;
  checkedAt: string | null;
  error: string | null;
  defaultBranch: string;
  sourceHeadSha: string | null;
  sourceAhead: boolean | null;
  createdAt: string;
}

export type DownloadStatus = 'queued' | 'downloading' | 'completed' | 'failed' | 'cancelled' | 'installing' | 'installed';
export interface DownloadRecord {
  id: string;
  projectId: string;
  projectName: string;
  assetId: number;
  fileName: string;
  tag: string;
  url: string;
  kind: AssetKind;
  totalBytes: number;
  receivedBytes: number;
  status: DownloadStatus;
  path: string | null;
  sha256: string | null;
  verified: boolean;
  error: string | null;
  createdAt: string;
  updatedAt: string;
}

export type BuildTaskStatus = 'queued' | 'claimed' | 'running' | 'succeeded' | 'failed' | 'cancelled';
export interface BuildTask {
  id: string;
  projectId: string;
  projectName: string;
  repository: string;
  sourceRef: string;
  sourceCommit: string | null;
  target: 'windows-x64';
  instructions: string;
  status: BuildTaskStatus;
  agentName: string | null;
  progress: string | null;
  artifactUrl: string | null;
  artifactSha256: string | null;
  error: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Settings {
  theme: Theme;
  checkIntervalMinutes: number;
  notifications: boolean;
  autoDownload: boolean;
  maxConcurrentDownloads: number;
  closeToTray: boolean;
  agentEnabled: boolean;
  agentPort: number;
}

export interface RuntimeInfo {
  appVersion: string;
  dataDir: string;
  portable: boolean;
  githubTokenConfigured: boolean;
  agent: {
    enabled: boolean;
    address: string | null;
    tokenConfigured: boolean;
    error: string | null;
  };
}

export interface Snapshot {
  projects: Project[];
  downloads: DownloadRecord[];
  buildTasks: BuildTask[];
  settings: Settings;
  runtime: RuntimeInfo;
}

export interface AddProjectInput {
  repository: string;
  channel: Channel;
  preference: AssetPreference;
  installedVersion?: string;
  installedPath?: string;
}

export interface ProjectChanges {
  channel?: Channel;
  preference?: AssetPreference;
  assetPattern?: string | null;
  installedVersion?: string | null;
  installedPath?: string | null;
  selectedAssetId?: number | null;
}

export interface InstallResult {
  kind: 'installer' | 'portable';
  message: string;
  path: string;
  installed: boolean;
}

export interface CheckResult {
  checked: number;
  updates: number;
  errors: number;
}

export interface AgentConnection {
  address: string;
  token: string;
}

export interface ManagedVersion {
  downloadId: string;
  version: string;
  path: string;
  active: boolean;
}
