import { describe, expect, it } from 'vitest';
import { assetKindLabel, canActivateManagedVersion, canDownloadAsset, canInstallAsset, canInstallDownload, downloadPercent, formatBytes, getDownloadStatus, getProjectStatus, hasProjectUpdate, isFilesProject, matchesProject, needsProjectAttention, parseRepository, projectPresentation, readableError, safeGithubUrl } from './ui';
import { createPreviewManagedVersions, createPreviewSnapshot } from './preview';
import { applyDownloadProgress, isDownloadRecord, mergeSnapshotDownloads } from './download-state';
import type { DownloadRecord, FileStatus, Project, ReleaseAsset } from './types';

describe('GitHub input and host-bound links', () => {
  it('accepts a repository name and normalizes supported project links', () => {
    expect(parseRepository(' Lake1059/FFmpegFreeUI ')).toBe('Lake1059/FFmpegFreeUI');
    expect(parseRepository('https://github.com/Predidit/Kazumi.git')).toBe('Predidit/Kazumi');
    expect(parseRepository('https://github.com/RoutineMiku/apkstart/releases/tag/v0.3.2')).toBe('RoutineMiku/apkstart');
  });
  it.each([
    'https://github.com.evil.example/owner/repo', 'http://github.com/owner/repo',
    'https://user@github.com/owner/repo', 'https://github.com:8443/owner/repo',
    'https://example.com/owner/repo', 'javascript:alert(1)', 'owner/repo/extra',
    'https://github.com/owner/repo/tree/main', '',
  ])('rejects unsafe or unsupported project input: %s', (input) => {
    expect(parseRepository(input)).toBeNull();
  });
  it('forwards only credential-free HTTPS GitHub links', () => {
    expect(safeGithubUrl('/owner/repo/releases', 'https://github.com/owner/repo')).toBe('https://github.com/owner/repo/releases');
    expect(safeGithubUrl('https://github.com/owner/repo/actions/runs/123')).toBe('https://github.com/owner/repo/actions/runs/123');
    expect(safeGithubUrl('https://github.com@evil.example/a')).toBeNull();
    expect(safeGithubUrl('file:///C:/Windows/System32/cmd.exe')).toBeNull();
    expect(safeGithubUrl('#content', 'https://github.com/owner/repo')).toBeNull();
  });
});

describe('managed version confirmation', () => {
  const versions = createPreviewManagedVersions('preview-ff');
  it('requires explicit confirmation before selecting a different managed version', () => {
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[1], false)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[1], true)).toBe(true);
  });
  it('does not offer switching for external, missing, or already-current versions', () => {
    expect(canActivateManagedVersion({ installedSource: 'manual' }, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: null }, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion(null, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, undefined, true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[0], true)).toBe(false);
  });
  it('keeps preview versions isolated and consistent with the sample installed records', () => {
    const preview = createPreviewSnapshot();
    const project = preview.projects.find((item) => item.id === 'preview-ff')!;
    expect(createPreviewManagedVersions('another-project')).toEqual([]);
    expect(versions.filter((version) => version.active)).toHaveLength(1);
    expect(project.installedSource).toBe('managed');
    expect(project.installedVersion).toBe(versions[0].version);
    expect(project.installedPath).toBe(versions[0].path);
    for (const version of versions) {
      expect(preview.downloads.find((download) => download.id === version.downloadId)).toMatchObject({ projectId: project.id, tag: version.version, status: 'installed', kind: 'portable' });
    }
    const changedPreview = createPreviewManagedVersions('preview-ff');
    changedPreview[1].active = true;
    expect(createPreviewManagedVersions('preview-ff')[1].active).toBe(false);
  });
});

describe('incremental download state', () => {
  const time = (offset: number) => new Date(Date.parse('2026-11-14T00:00:00.000Z') + offset).toISOString();
  function fixture() {
    const snapshot = createPreviewSnapshot();
    const index = snapshot.downloads.findIndex((record) => record.status === 'downloading');
    const record: DownloadRecord = { ...snapshot.downloads[index], receivedBytes: 25, totalBytes: 100, createdAt: time(-1000), updatedAt: time(0) };
    snapshot.downloads[index] = record;
    return { snapshot, record, index };
  }
  it('accepts complete records and rejects malformed events before replacing UI data', () => {
    const { record } = fixture();
    expect(isDownloadRecord(record)).toBe(true);
    for (const invalid of [null, [], {}, { ...record, projectId: '' }, { ...record, status: 'success' }, { ...record, path: 42 }, { ...record, receivedBytes: Number.NaN }, { ...record, totalBytes: -1 }, { ...record, updatedAt: 'not-a-date' }]) {
      expect(isDownloadRecord(invalid)).toBe(false);
    }
  });
  it('replaces only the matching download and preserves every other list/settings reference', () => {
    const { snapshot, record, index } = fixture();
    const progress = { ...record, receivedBytes: 50, updatedAt: time(400) };
    const next = applyDownloadProgress(snapshot, progress)!;
    expect(next.downloads[index]).toBe(progress);
    expect(next.projects).toBe(snapshot.projects);
    expect(next.buildTasks).toBe(snapshot.buildTasks);
    expect(next.settings).toBe(snapshot.settings);
    expect(next.runtime).toBe(snapshot.runtime);
    snapshot.downloads.forEach((entry, i) => { if (i !== index) expect(next.downloads[i]).toBe(entry); });
    expect(snapshot.downloads[index]).toBe(record);
    expect(record.receivedBytes).toBe(25);
  });
  it('ignores older and same-clock events without allocating a new state', () => {
    const { snapshot, record } = fixture();
    expect(applyDownloadProgress(snapshot, { ...record, receivedBytes: 10, updatedAt: time(-1) })).toBe(snapshot);
    expect(applyDownloadProgress(snapshot, { ...record, receivedBytes: 50 })).toBe(snapshot);
  });
  it('keeps cancellation against late progress and accepts a newer retry with reset bytes', () => {
    const { snapshot, record, index } = fixture();
    const cancelled: DownloadRecord = { ...record, status: 'cancelled', error: '下载已取消，可重试', updatedAt: time(400) };
    const stopped = applyDownloadProgress(snapshot, cancelled)!;
    expect(stopped.downloads[index]).toBe(cancelled);
    expect(applyDownloadProgress(stopped, { ...record, receivedBytes: 80, updatedAt: time(200) })).toBe(stopped);
    const retry: DownloadRecord = { ...record, status: 'queued', receivedBytes: 0, error: null, updatedAt: time(800) };
    expect(applyDownloadProgress(stopped, retry)!.downloads[index]).toBe(retry);
  });
  it('requests resync for unknown or mismatched identities, but keeps unsubscribed download history usable', () => {
    const { snapshot, record, index } = fixture();
    expect(applyDownloadProgress(snapshot, { ...record, id: 'unknown-download', updatedAt: time(400) })).toBeNull();
    expect(applyDownloadProgress(snapshot, { ...record, projectId: 'unknown-project', updatedAt: time(400) })).toBeNull();
    const orphan = { ...snapshot, projects: [] };
    expect(applyDownloadProgress(orphan, { ...record, receivedBytes: 50, updatedAt: time(400) })!.downloads[index].receivedBytes).toBe(50);
  });
  it('keeps in-flight progress when a slower full read returns, while accepting its metadata', () => {
    const { snapshot, record, index } = fixture();
    const progress = { ...record, receivedBytes: 75, updatedAt: time(800) };
    const current = applyDownloadProgress(snapshot, progress)!;
    const incoming = { ...snapshot, settings: { ...snapshot.settings, notifications: false } };
    const merged = mergeSnapshotDownloads(current, incoming, snapshot);
    expect(merged.downloads[index]).toBe(progress);
    expect(merged.settings).toBe(incoming.settings);
    expect(merged.projects).toBe(incoming.projects);
    const tied = { ...incoming, downloads: incoming.downloads.map((item) => item.id === record.id ? { ...item, updatedAt: time(800) } : item) };
    expect(mergeSnapshotDownloads(current, tied, snapshot).downloads[index]).toBe(progress);
  });
  it('accepts authoritative same-tick retry metadata when no newer event arrived during the read', () => {
    const { snapshot, record, index } = fixture();
    const cancelled: DownloadRecord = { ...record, status: 'cancelled', updatedAt: time(400) };
    const current = applyDownloadProgress(snapshot, cancelled)!;
    const retry: DownloadRecord = { ...cancelled, status: 'queued', receivedBytes: 0 };
    expect(applyDownloadProgress(current, retry)).toBe(current);
    const incoming = { ...current, downloads: current.downloads.map((item) => item.id === retry.id ? retry : item) };
    expect(mergeSnapshotDownloads(current, incoming, current).downloads[index]).toBe(retry);
  });
  it('accepts newer completion metadata and never re-adds records absent from a full read', () => {
    const { snapshot, record } = fixture();
    const current = applyDownloadProgress(snapshot, { ...record, receivedBytes: 50, updatedAt: time(400) })!;
    const incoming = { ...snapshot, downloads: snapshot.downloads.map((item) => item.id === record.id ? { ...item, status: 'completed' as const, receivedBytes: 100, updatedAt: time(800) } : item) };
    expect(mergeSnapshotDownloads(current, incoming, snapshot)).toBe(incoming);
    const removed = { ...incoming, downloads: [] };
    expect(mergeSnapshotDownloads(current, removed, snapshot)).toBe(removed);
    expect(mergeSnapshotDownloads(null, incoming, null)).toBe(incoming);
  });
});

describe('software and file resource capabilities', () => {
  const software = createPreviewSnapshot().projects[0];
  const files: Project = { ...software, managementMode: 'files', fileStatus: 'notDownloaded' };
  const legacyAsset = software.release!.assets[0];
  const plainZip: ReleaseAsset = { ...legacyAsset, name: 'reference-data.zip', kind: 'portable', compatible: false, downloadable: true, installable: false };

  it('keeps legacy projects in software mode and separates file subscriptions', () => {
    expect(isFilesProject(software)).toBe(false);
    expect(isFilesProject({ managementMode: 'software' })).toBe(false);
    expect(isFilesProject(files)).toBe(true);
  });
  it('downloads ordinary files even when they are not compatible software', () => {
    expect(canDownloadAsset(plainZip)).toBe(true);
    expect(canInstallAsset(plainZip, software)).toBe(false);
    expect(assetKindLabel(plainZip.kind, canInstallAsset(plainZip, software), plainZip.name)).toBe('普通文件');
  });
  it('uses explicit download flags rather than the old compatible flag', () => {
    expect(canDownloadAsset({ compatible: true, downloadable: false })).toBe(false);
    expect(canDownloadAsset({ compatible: false, downloadable: true })).toBe(true);
    expect(canDownloadAsset({ compatible: true })).toBe(true);
    expect(canDownloadAsset({ compatible: false })).toBe(false);
  });
  it('keeps every downloadable attachment available within the same release', () => {
    const attachments: ReleaseAsset[] = [
      { ...plainZip, id: 601, name: 'reference-data.zip' },
      { ...plainZip, id: 602, name: 'guide.pdf', kind: 'other' },
      { ...plainZip, id: 603, name: 'over-limit.zip', size: 4 * 1024 ** 3 + 1, downloadable: false },
    ];
    expect(attachments.filter(canDownloadAsset).map((asset) => asset.id)).toEqual([601, 602]);
    expect(attachments.map((asset) => canInstallAsset(asset, files))).toEqual([false, false, false]);
  });
  it('never installs in file mode, including old compatible ZIP fixtures', () => {
    expect(canInstallAsset(legacyAsset, software)).toBe(true);
    expect(canInstallAsset({ ...legacyAsset, installable: true }, files)).toBe(false);
    expect(canInstallAsset(legacyAsset, files)).toBe(false);
    expect(canInstallAsset({ ...legacyAsset, kind: 'other', installable: true }, software)).toBe(false);
    expect(canActivateManagedVersion(files, createPreviewManagedVersions(software.id)[1], true)).toBe(false);
  });
  it('honors false installable on portable and installer download records', () => {
    expect(canInstallDownload({ kind: 'portable', installable: false })).toBe(false);
    expect(canInstallDownload({ kind: 'installer', installable: false })).toBe(false);
    expect(canInstallDownload({ kind: 'other', installable: true })).toBe(false);
  });
  it('retains legacy software installation and labels unknown kinds as ordinary files', () => {
    expect(canInstallDownload({ kind: 'portable' })).toBe(true);
    expect(canInstallDownload({ kind: 'installer' })).toBe(true);
    expect(canInstallDownload({ kind: 'other' })).toBe(false);
    expect(assetKindLabel('future-kind', true)).toBe('普通文件');
    expect(assetKindLabel('portable', false, 'data.zip')).toBe('普通文件');
    expect(assetKindLabel('portable', true, 'app.EXE')).toBe('单文件免安装');
    expect(assetKindLabel('installer', true)).toBe('安装程序');
  });
  it.each<[FileStatus, string]>([
    ['notDownloaded', '未下载'], ['downloaded', '已下载'], ['updateAvailable', '有新版本'], ['missing', '文件已移走'],
  ])('shows the file state %s without installed-version wording', (fileStatus, label) => {
    expect(getProjectStatus({ ...files, fileStatus, status: 'versionUnknown' }).label).toBe(label);
  });
  it('preserves checking and error states while avoiding software-only warnings for files', () => {
    expect(getProjectStatus({ ...files, status: 'checking' }).label).toBe('检查中');
    expect(getProjectStatus({ ...files, status: 'error' }).label).toBe('检查失败');
    expect(getProjectStatus({ ...files, status: 'noRelease' }).label).toBe('暂无发行版');
    expect(getProjectStatus({ ...files, fileStatus: undefined, status: 'noCompatibleAsset' }).label).toBe('未下载');
    expect(getProjectStatus({ ...software, status: 'versionUnknown' }).label).toBe('未记录版本');
  });
  it('filters file updates using downloaded-file state, not stale software state', () => {
    expect(hasProjectUpdate({ ...files, status: 'updateAvailable', fileStatus: 'downloaded' })).toBe(false);
    expect(hasProjectUpdate({ ...files, status: 'upToDate', fileStatus: 'updateAvailable' })).toBe(true);
    expect(hasProjectUpdate({ ...software, status: 'updateAvailable' })).toBe(true);
    expect(needsProjectAttention({ ...files, status: 'versionUnknown' })).toBe(false);
    expect(needsProjectAttention({ ...files, fileStatus: 'missing' })).toBe(true);
    expect(needsProjectAttention({ ...files, status: 'error' })).toBe(true);
  });
  it('labels saved files as downloaded rather than waiting for installation', () => {
    expect(getDownloadStatus({ kind: 'portable', installable: false, status: 'completed' }).label).toBe('已下载');
    expect(getDownloadStatus({ kind: 'installer', installable: false, status: 'installed' }).label).toBe('已下载');
    expect(getDownloadStatus({ kind: 'other', status: 'completed' }).label).toBe('已下载');
    expect(getDownloadStatus({ kind: 'portable', status: 'completed' }).label).toBe('已下载 · 待安装');
    expect(getDownloadStatus({ kind: 'portable', status: 'installed' }).label).toBe('已安装');
    expect(getDownloadStatus({ kind: 'other', status: 'failed' }).label).toBe('下载失败');
  });
});

describe('readable list states', () => {
  it('searches all words across names and repository identity', () => {
    const project = createPreviewSnapshot().projects[0];
    expect(matchesProject(project, 'lake1059 ffmpeg')).toBe(true);
    expect(matchesProject(project, 'kazumi')).toBe(false);
    expect(matchesProject(project, '  ')).toBe(true);
  });
  it('uses only optional note text when either note field is present', () => {
    const project = createPreviewSnapshot().projects[0];
    const note = { repository: `${project.owner}/${project.repo}`, name: ' 转码工具 ', description: ' 常用视频工具 ', updatedAt: new Date().toISOString() };
    expect(projectPresentation(project)).toMatchObject({ title: project.name, annotated: false, label: project.name });
    expect(projectPresentation(project, note)).toEqual({ title: '转码工具', description: '常用视频工具', annotated: true, label: '转码工具' });
    expect(projectPresentation(project, { ...note, name: '' })).toEqual({ title: null, description: '常用视频工具', annotated: true, label: '常用视频工具' });
    expect(projectPresentation(project, { ...note, description: '' })).toEqual({ title: '转码工具', description: null, annotated: true, label: '转码工具' });
    expect(projectPresentation(project, { ...note, name: ' ', description: '' })).toMatchObject({ title: project.name, annotated: false });
    expect(matchesProject(project, '转码 常用', note)).toBe(true);
    expect(matchesProject(project, `${project.owner} ${project.repo}`, note)).toBe(true);
  });
  it('does not invent progress when the total is unknown', () => {
    expect(downloadPercent({ receivedBytes: 20, totalBytes: 0 })).toBe(0);
    expect(downloadPercent({ receivedBytes: 30, totalBytes: 100 })).toBe(30);
    expect(downloadPercent({ receivedBytes: 110, totalBytes: 100 })).toBe(100);
    expect(downloadPercent({ receivedBytes: Number.NaN, totalBytes: 100 })).toBe(0);
    expect(downloadPercent({ receivedBytes: -20, totalBytes: 100 })).toBe(0);
  });
  it('formats sizes without NaN output', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('大小未知');
  });
  it('never prints a credential-like value or serializes arbitrary error objects', () => {
    expect(readableError(new Error(`failed: ${'ghp_' + 'x'.repeat(30)}`))).toBe('failed: 〔已隐藏的凭据〕');
    expect(readableError({ token: 'not-a-real-token' })).toBe('操作未完成，请稍后重试。');
  });
  it('keeps Agent disabled and secrets absent in development fixtures', () => {
    const preview = createPreviewSnapshot();
    expect(preview.settings.agentEnabled).toBe(false);
    expect(preview.runtime.agent.enabled).toBe(false);
    expect(preview.runtime.githubTokenConfigured).toBe(false);
    expect(preview.runtime.agent).not.toHaveProperty('token');
  });
});
