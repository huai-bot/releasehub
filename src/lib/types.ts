export type Channel = 'stable' | 'preview';
export type AssetPreference = 'auto' | 'portable' | 'installer';
export type Theme = 'system' | 'light' | 'dark';
export type ColorScheme = 'blue' | 'teal' | 'violet' | 'amber';
export type Material = 'acrylic' | 'mica';
export type ManagementMode = 'software' | 'files';
export type FileStatus = 'notDownloaded' | 'downloaded' | 'updateAvailable' | 'missing';
export type LibrarySort = 'added' | 'name' | 'nameDesc' | 'released' | 'updated';
export type ProjectStatus = 'local' | 'notChecked' | 'checking' | 'upToDate' | 'updateAvailable' | 'versionUnknown' | 'noRelease' | 'noCompatibleAsset' | 'error';
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
  downloadable?: boolean;
  installable?: boolean;
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
  /** Missing in older records; backups are opt-in per software. */
  backupEnabled?: boolean;
  updatesMuted?: boolean;
  managementMode?: ManagementMode;
  fileStatus?: FileStatus;
  id: string;
  owner: string;
  repo: string;
  repositoryMatch?: string | null;
  name: string;
  description: string;
  url: string;
  avatarUrl: string | null;
  channel: Channel;
  preference: AssetPreference;
  assetPattern: string | null;
  installedVersion: string | null;
  installedPath: string | null;
  executablePath?: string | null;
  installedSource: 'manual' | 'managed' | 'installer' | 'executable' | null;
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

export type DownloadStatus = 'queued' | 'downloading' | 'external' | 'completed' | 'failed' | 'cancelled' | 'installing' | 'installed' | 'deleted';
export interface DownloadRecord {
  installable?: boolean;
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

export type LibraryLayout = 'two' | 'three' | 'list';
export type WorkspacePage = 'home' | 'library' | 'updates' | 'downloads' | 'github' | 'trainers' | 'agent';
export type GithubTab = 'explore' | 'owned' | 'starred' | 'activity' | 'feed';
export interface ProxySettings {
  mode: 'system' | 'manual' | 'direct';
  url: string;
  username: string;
}
export interface ProxyTestResult {
  ok: boolean;
  mode: string;
  source: string;
  target: string;
  status: number | null;
  latencyMs: number;
  message: string;
  warning?: string | null;
  retryAt?: string | null;
}
export interface RepositoryNote {
  repository: string;
  name: string;
  description: string;
  updatedAt: string;
}

export interface ExternalDownloader {
  name: string;
  endpoint: string;
  requestTemplate: string;
}

export interface Settings {
  theme: Theme;
  colorScheme?: ColorScheme;
  material?: Material;
  blur?: boolean;
  libraryLayout?: LibraryLayout;
  librarySort?: LibrarySort;
  startupPage: WorkspacePage;
  startupGithubTab: GithubTab;
  navigationOrder: WorkspacePage[];
  proxy: ProxySettings;
  checkIntervalMinutes: number;
  notifications: boolean;
  autoDownload: boolean;
  maxConcurrentDownloads: number;
  downloadConnections?: number;
  useExternalDownloader?: boolean;
  externalDownloader?: ExternalDownloader;
  closeToTray: boolean;
  agentEnabled: boolean;
  agentPort: number;
}

export interface RuntimeInfo {
  appVersion: string;
  dataDir: string;
  portable: boolean;
  githubTokenConfigured: boolean;
  proxyPasswordConfigured: boolean;
  agent: {
    enabled: boolean;
    address: string | null;
    tokenConfigured: boolean;
    error: string | null;
  };
}

export interface Snapshot {
  readUpdateTags?: Record<string, string>;
  projects: Project[];
  downloads: DownloadRecord[];
  installations?: InstallationCheck[];
  buildTasks: BuildTask[];
  repositoryNotes: RepositoryNote[];
  settings: Settings;
  runtime: RuntimeInfo;
}

export interface AddProjectInput {
  managementMode?: ManagementMode;
  repository: string;
  channel: Channel;
  preference: AssetPreference;
  installedVersion?: string;
  installedPath?: string;
}

export interface ProjectChanges {
  backupEnabled?: boolean;
  updatesMuted?: boolean;
  managementMode?: ManagementMode;
  channel?: Channel;
  preference?: AssetPreference;
  assetPattern?: string | null;
  installedVersion?: string | null;
  installedPath?: string | null;
  executablePath?: string | null;
  selectedAssetId?: number | null;
}

export interface InstallResult {
  installationId?: string;
  kind: 'installer' | 'portable';
  message: string;
  path: string;
  installed: boolean;
}

export interface InstallPreview {
  downloadId: string;
  projectId: string;
  targetPath: string;
  backupEnabled: boolean;
  backupPath: string | null;
  previousVersion: string | null;
  nextVersion: string;
  backupBytes: number;
  packageBytes: number;
  availableBytes: number;
  kind: 'installer' | 'portable';
  archiveRoots: string[];
  suggestedRoot: string | null;
  requiresRootSelection: boolean;
  legacyManaged: boolean;
}

export interface BackupRecord {
  projectId: string;
  downloadId: string;
  version: string | null;
  installedSource: 'manual' | 'managed' | 'installer' | 'executable' | null;
  targetPath: string;
  path: string;
  createdAt: string;
  bytes: number;
  kind: 'installer' | 'portable';
  phase: 'ready' | 'installing' | 'restoring' | 'recoveryRequired';
  error: string | null;
}

export interface CheckResult {
  checked: number;
  updates: number;
  unreadUpdates: number;
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

/** Contains launch configuration only, never a credential. */
export interface AgentIntegration {
  command: string;
  args: string[];
  claudeConfig: string;
  codexConfig: string;
  genericConfig: string;
  cursorConfig: string;
  vscodeConfig: string;
  cliCommand: string;
  tools: { name: string; description: string }[];
  readOnly: true;
}

export interface GithubAccount {
  id?: number;
  login: string;
  name: string | null;
  avatarUrl: string | null;
  url: string;
}

export interface GithubLogin {
  state: 'waiting' | 'complete' | 'error';
  userCode: string | null;
  verificationUri: string;
  message: string | null;
}

export interface GithubStatus {
  available: boolean;
  connected: boolean;
  account: GithubAccount | null;
  login: GithubLogin | null;
  error: string | null;
  provider: 'github-oauth' | 'github-cli';
  verified?: boolean;
  sessionRevision?: number;
  scopes?: string[];
}

export type GithubRepositoryTab = 'explore' | 'starred' | 'owned';

export interface GithubRepository {
  id: number;
  name: string;
  fullName: string;
  owner: string;
  description: string | null;
  url: string;
  private: boolean;
  archived: boolean;
  fork: boolean;
  language: string | null;
  stars: number;
  forks: number;
  openIssues: number;
  hasIssues?: boolean;
  hasDiscussions?: boolean;
  updatedAt: string;
  defaultBranch: string;
  homepage: string | null;
  topics: string[];
  canAdmin: boolean;
  starred: boolean | null;
}

export interface GithubRepositoryPage {
  items: GithubRepository[];
  page: number;
  hasMore: boolean;
  totalCount: number | null;
}

export interface GithubRepositoryDetail {
  repository: GithubRepository;
  readme: string;
}

export interface GithubActivity {
  id: string;
  actor: string;
  avatarUrl: string | null;
  repository: string;
  type: string;
  summary: string;
  url: string;
  createdAt: string;
}

export interface GithubActivityPage {
  items: GithubActivity[];
  page: number;
  hasMore: boolean;
}

export interface GithubUserList {
  id: string;
  name: string;
  description: string | null;
  url: string;
  itemCount: number;
}
export interface GithubListRepositoryPage {
  items: GithubRepository[];
  endCursor: string | null;
  hasMore: boolean;
  totalCount: number;
}
export interface GithubFeedItem {
  id: string;
  actor: string;
  avatarUrl: string | null;
  summary: string;
  body: string;
  url: string;
  repository: string | null;
  occurredAt: string | null;
}
export interface GithubFeedPage {
  cached?: boolean;
  /** A cookie store from an earlier website login exists; validate silently, never trust as connected. */
  remembered?: boolean;
  connected: boolean;
  loading: boolean;
  account: string | null;
  items: GithubFeedItem[];
  hasMore: boolean;
  lastUpdated: string | null;
  error: string | null;
  sourceUrl: string;
  requestId: string;
  sessionRevision: number;
}

/** A machine-local projection; contains neither credentials nor raw API headers. */
export interface GithubCacheEntry<T = unknown> {
  data: T;
  updatedAt: number;
  fresh: boolean;
  sessionRevision: number;
}
export interface GithubCacheStats { bytes: number; entries: number }
export type GithubCacheOperation = 'github_repositories' | 'github_repository' | 'github_activity' | 'github_lists' | 'github_list_repositories' | 'github_repository_lists';
export type InstallationStatus = 'waiting' | 'detecting' | 'verified' | 'manual' | 'rebootRequired' | 'unconfirmed' | 'cancelled' | 'failed' | 'superseded';
export interface InstallationCheck {
  id: string;
  projectId: string;
  downloadId: string;
  expectedVersion: string;
  previousVersion: string | null;
  status: InstallationStatus;
  startedAt: string;
  checkedAt: string | null;
  detectedVersion: string | null;
  source: string | null;
  message: string;
}
