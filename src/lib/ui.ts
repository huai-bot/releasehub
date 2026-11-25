import type { BuildTaskStatus, DownloadRecord, DownloadStatus, FileStatus, ManagedVersion, Project, ProjectStatus, ReleaseAsset, RepositoryNote } from './types';

export type Page = 'home' | 'library' | 'updates' | 'downloads' | 'github' | 'agent' | 'settings';
export type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'accent';
export interface StatusLabel { label: string; tone: Tone }

export const projectStatus: Record<ProjectStatus, StatusLabel> = {
  notChecked: { label: '待检查', tone: 'neutral' },
  checking: { label: '检查中', tone: 'accent' },
  upToDate: { label: '已是最新', tone: 'success' },
  updateAvailable: { label: '发现更新', tone: 'accent' },
  versionUnknown: { label: '未记录版本', tone: 'warning' },
  noRelease: { label: '暂无发行版', tone: 'neutral' },
  noCompatibleAsset: { label: '需选择安装包', tone: 'warning' },
  error: { label: '检查失败', tone: 'danger' },
};
export const downloadStatus: Record<DownloadStatus, StatusLabel> = {
  queued: { label: '等待下载', tone: 'neutral' },
  downloading: { label: '正在下载', tone: 'accent' },
  external: { label: '已交给外部下载器', tone: 'neutral' },
  completed: { label: '已下载 · 待安装', tone: 'success' },
  failed: { label: '下载失败', tone: 'danger' },
  cancelled: { label: '已取消', tone: 'neutral' },
  installing: { label: '处理中', tone: 'accent' },
  installed: { label: '已安装', tone: 'success' },
  deleted: { label: '安装包已删除', tone: 'neutral' },
};
export const fileStatus: Record<FileStatus, StatusLabel> = {
  notDownloaded: { label: '未下载', tone: 'neutral' },
  downloaded: { label: '已下载', tone: 'success' },
  updateAvailable: { label: '有新版本', tone: 'accent' },
  missing: { label: '文件已移走', tone: 'warning' },
};

export function isFilesProject(project: Pick<Project, 'managementMode'>): boolean {
  return project.managementMode === 'files';
}

/** Old snapshots used compatible for both capabilities. Explicit flags always win. */
export function canDownloadAsset(asset: Pick<ReleaseAsset, 'downloadable' | 'compatible'>): boolean {
  return asset.downloadable ?? asset.compatible;
}

export function canInstallAsset(asset: Pick<ReleaseAsset, 'installable' | 'compatible' | 'kind'>, project: Pick<Project, 'managementMode'>): boolean {
  return !isFilesProject(project) && (asset.kind === 'installer' || asset.kind === 'portable') && (asset.installable ?? asset.compatible);
}

export function canInstallDownload(download: Pick<DownloadRecord, 'installable' | 'kind'>): boolean {
  return (download.kind === 'installer' || download.kind === 'portable') && (download.installable ?? true);
}

export function assetKindLabel(kind: string, installable: boolean, name = ''): string {
  if (!installable) return '普通文件';
  if (kind === 'installer') return '安装程序';
  if (kind === 'portable') return name.toLowerCase().endsWith('.exe') ? '单文件免安装' : '免安装压缩包';
  return '普通文件';
}

export function getProjectStatus(project: Pick<Project, 'managementMode' | 'fileStatus' | 'status'>): StatusLabel {
  if (!isFilesProject(project) || ['notChecked', 'checking', 'noRelease', 'error'].includes(project.status)) return projectStatus[project.status];
  return fileStatus[project.fileStatus ?? 'notDownloaded'];
}

export function hasProjectUpdate(project: Pick<Project, 'managementMode' | 'fileStatus' | 'status' | 'updatesMuted'>): boolean {
  return !project.updatesMuted && (isFilesProject(project) ? project.fileStatus === 'updateAvailable' : project.status === 'updateAvailable');
}

export function hasUnreadUpdate(project: Project, readTags?: Record<string, string>): boolean {
  return hasProjectUpdate(project) && !!project.release && readTags?.[project.id] !== project.release.tag;
}

export function needsProjectAttention(project: Pick<Project, 'managementMode' | 'fileStatus' | 'status'>): boolean {
  return ['error', 'noCompatibleAsset'].includes(project.status) || (isFilesProject(project) ? project.fileStatus === 'missing' : project.status === 'versionUnknown');
}

export function getDownloadStatus(download: Pick<DownloadRecord, 'status' | 'kind' | 'installable'>): StatusLabel {
  if (!canInstallDownload(download) && ['completed', 'installed'].includes(download.status)) return { label: '已下载', tone: 'success' };
  if (download.status === 'deleted' && !canInstallDownload(download)) return { label: '下载文件已删除', tone: 'neutral' };
  return downloadStatus[download.status];
}

export const buildStatus: Record<BuildTaskStatus, StatusLabel> = {
  queued: { label: '等待 Agent', tone: 'neutral' },
  claimed: { label: '已领取', tone: 'accent' },
  running: { label: '构建中', tone: 'accent' },
  succeeded: { label: '构建完成', tone: 'success' },
  failed: { label: '构建失败', tone: 'danger' },
  cancelled: { label: '已取消', tone: 'neutral' },
};

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '大小未知';
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let size = bytes / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size >= 100 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`;
}

export function formatDate(value: string | null, includeTime = false): string {
  if (!value || Number.isNaN(Date.parse(value))) return '尚未记录';
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'short', day: 'numeric', ...(includeTime ? { hour: '2-digit', minute: '2-digit' } as const : {}),
  }).format(new Date(value));
}

export function downloadPercent(download: Pick<DownloadRecord, 'receivedBytes' | 'totalBytes'>): number {
  if (!Number.isFinite(download.totalBytes) || download.totalBytes <= 0 || !Number.isFinite(download.receivedBytes)) return 0;
  return Math.max(0, Math.min(100, Math.round(download.receivedBytes / download.totalBytes * 100)));
}

export function canActivateManagedVersion(project: Pick<Project, 'installedSource' | 'managementMode'> | null, version: ManagedVersion | undefined, confirmed: boolean): boolean {
  return Boolean(confirmed && project && !isFilesProject(project) && project.installedSource === 'managed' && version && !version.active);
}

export function projectPresentation(project: Pick<Project, 'name' | 'description'>, note?: RepositoryNote | null): { title: string | null; description: string | null; annotated: boolean; label: string } {
  const title = note?.name.trim() ?? '';
  const description = note?.description.trim() ?? '';
  if (title || description) return { title: title || null, description: description || null, annotated: true, label: title || description };
  return { title: project.name, description: project.description || null, annotated: false, label: project.name };
}

export function matchesProject(project: Project, query: string, note?: RepositoryNote | null): boolean {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const haystack = `${project.name} ${project.owner}/${project.repo} ${project.description} ${note?.name ?? ''} ${note?.description ?? ''}`.toLocaleLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function parseRepository(input: string): string | null {
  let value = input.trim();
  if (/^https?:\/\//i.test(value)) {
    const safe = safeGithubUrl(value);
    if (!safe) return null;
    const path = new URL(safe).pathname.split('/').filter(Boolean);
    if (path.length < 2 || (path.length > 2 && path[2] !== 'releases')) return null;
    value = `${path[0]}/${path[1]}`;
  }
  value = value.replace(/\/$/, '').replace(/\.git$/i, '');
  return /^[a-z\d](?:[a-z\d-]{0,38})\/[a-z\d_.-]{1,100}$/i.test(value) ? value : null;
}

/** Match the host's GitHub-only browser policy before forwarding a link. */
export function safeGithubUrl(value: string, base?: string): string | null {
  if (!value || value.trim().startsWith('#')) return null;
  try {
    const url = new URL(value, base);
    if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.username || url.password || url.port) return null;
    return url.href;
  } catch { return null; }
}

export function readableError(error: unknown): string {
  const message = typeof error === 'string' ? error : error instanceof Error ? error.message : '操作未完成，请稍后重试。';
  return message.replace(/(?:github_pat_|gh[pousr]_)[A-Za-z0-9_]+/g, '〔已隐藏的凭据〕').slice(0, 800);
}

export function isDownloadRunning(download: Pick<DownloadRecord, 'status'>): boolean {
  return ['queued', 'downloading', 'installing'].includes(download.status);
}

export function canDeleteDownload(download: Pick<DownloadRecord, 'status' | 'path'>): boolean {
  return !isDownloadRunning(download) && download.status !== 'deleted' && Boolean(download.path);
}

export function canRetryDownload(download: Pick<DownloadRecord, 'status'>): boolean {
  return ['failed', 'cancelled', 'deleted'].includes(download.status);
}

export function pendingInstallDownloads(downloads: DownloadRecord[], projects?: Project[]): DownloadRecord[] {
  return downloads.filter((download) => download.status === 'completed' && canInstallDownload(download)
    && !projects?.some((project) => project.id === download.projectId && (isFilesProject(project) || project.installedVersion === download.tag)));
}

export function selectedDownloadAsset(project: Project): ReleaseAsset | undefined {
  return project.release?.assets.find((asset) => asset.id === project.selectedAssetId && canDownloadAsset(asset));
}

/** Plain-text preview only. Full rich notes remain in the project detail. */
export function releaseSummary(body: string, limit = 220): string {
  const text = body.replace(/!\[[^\]]*\]\([^)]*\)/g, '').replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/<[^>]*>/g, '').replace(/[`#*>_~]/g, '').replace(/\s+/g, ' ').trim();
  return text.length > limit ? `${text.slice(0, limit).trimEnd()}…` : text;
}

export function projectInitials(project: Pick<Project, 'name' | 'repo'>): string {
  return (project.name || project.repo).replace(/[^A-Za-z\d\u4e00-\u9fff]/g, '').slice(0, 2).toUpperCase() || 'GH';
}
