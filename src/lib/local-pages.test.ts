import { describe, expect, it } from 'vitest';
import type { DownloadRecord } from './types';
import { applyDownloadProgress, isDownloadRecord, mergeSnapshotDownloads } from './download-state';
import { createPreviewSnapshot } from './preview';
import { canDeleteDownload, canRetryDownload, getDownloadStatus, isDownloadRunning, pendingInstallDownloads, releaseSummary, selectedDownloadAsset } from './ui';

const snapshot = () => createPreviewSnapshot();
const completed = (): DownloadRecord => snapshot().downloads.find((record) => record.id === 'preview-dl1')!;

describe('Home pending installs and update attachments', () => {
  it('offers only completed software packages, not saved ordinary files or already installed tags', () => {
    const data = snapshot();
    expect(pendingInstallDownloads(data.downloads, data.projects).map((record) => record.id)).toEqual(['preview-dl1']);
    const installed = data.projects.map((project) => project.id === 'preview-ff' ? { ...project, installedVersion: '6.5.0' } : project);
    expect(pendingInstallDownloads(data.downloads, installed)).toEqual([]);
    const fileMode = data.projects.map((project) => project.id === 'preview-ff' ? { ...project, managementMode: 'files' as const } : project);
    expect(pendingInstallDownloads(data.downloads, fileMode)).toEqual([]);
  });
  it('does not require a subscription to retain an orphaned package in downloads', () => {
    expect(pendingInstallDownloads([completed()], []).map((record) => record.id)).toEqual(['preview-dl1']);
    expect(pendingInstallDownloads([{ ...completed(), installable: false }])).toEqual([]);
    expect(pendingInstallDownloads([{ ...completed(), status: 'deleted' }])).toEqual([]);
  });
  it('uses the explicitly selected downloadable attachment rather than falling back to a helper program', () => {
    const project = snapshot().projects[0];
    expect(selectedDownloadAsset(project)?.id).toBe(101);
    expect(selectedDownloadAsset({ ...project, selectedAssetId: 102 })).toBeUndefined();
    expect(selectedDownloadAsset({ ...project, selectedAssetId: null })).toBeUndefined();
    const allowed = { ...project, release: { ...project.release!, assets: project.release!.assets.map((asset) => asset.id === 102 ? { ...asset, downloadable: true } : asset) }, selectedAssetId: 102 };
    expect(selectedDownloadAsset(allowed)?.id).toBe(102);
  });
  it('turns upstream Markdown into a bounded plain text summary', () => {
    expect(releaseSummary('## 改进\n\n- **修复** [说明](https://github.com/a/b) ![图](https://example.com/a.png) <b>界面</b>')).toBe('改进 - 修复 说明 界面');
    expect(releaseSummary('abcdefghijk', 8)).toBe('abcdefgh…');
    expect(releaseSummary('   ')).toBe('');
    expect(releaseSummary('<img src=x onerror=alert(1)>安全')).toBe('安全');
  });
});

describe('download package deletion and retry lifecycle', () => {
  it.each(['queued', 'downloading', 'installing'] as const)('blocks file deletion and retry for %s', (status) => {
    const record = { ...completed(), status };
    expect(isDownloadRunning(record)).toBe(true);
    expect(canDeleteDownload(record)).toBe(false);
    expect(canRetryDownload(record)).toBe(false);
  });
  it.each(['completed', 'installed', 'failed', 'cancelled'] as const)('deletes only a tracked local package for %s', (status) => {
    expect(canDeleteDownload({ ...completed(), status })).toBe(true);
    expect(canDeleteDownload({ ...completed(), status, path: null })).toBe(false);
  });
  it('recognizes legacy deleted records for migration and event handling', () => {
    const deleted = { ...completed(), status: 'deleted' as const, path: null };
    expect(isDownloadRecord(deleted)).toBe(true);
    expect(canDeleteDownload(deleted)).toBe(false);
    expect(canRetryDownload(deleted)).toBe(true);
    expect(getDownloadStatus(deleted)).toEqual({ label: '安装包已删除', tone: 'neutral' });
    expect(getDownloadStatus({ ...deleted, installable: false }).label).toBe('下载文件已删除');
    expect(getDownloadStatus({ ...deleted, status: 'installed' }).label).toBe('已安装');
  });
  it('preserves a newer deletion event against stale completion and accepts a later retry', () => {
    const data = snapshot();
    const record = data.downloads.find((item) => item.id === 'preview-dl1')!;
    record.updatedAt = '2026-11-14T00:00:00.000Z';
    const deleted = { ...record, status: 'deleted' as const, path: null, updatedAt: '2026-11-14T00:00:01.000Z' };
    const changed = applyDownloadProgress(data, deleted)!;
    expect(changed.projects).toBe(data.projects);
    expect(applyDownloadProgress(changed, record)).toBe(changed);
    expect(mergeSnapshotDownloads(changed, data, data).downloads.find((item) => item.id === record.id)).toBe(deleted);
    const retry = { ...deleted, status: 'queued' as const, receivedBytes: 0, updatedAt: '2026-11-14T00:00:02.000Z' };
    expect(applyDownloadProgress(changed, retry)!.downloads.find((item) => item.id === record.id)).toBe(retry);
  });
  it('keeps preview paths synthetic and provides pending/deleted examples for visual review', () => {
    const data = snapshot();
    expect(data.projects[0].executablePath).toContain('.\\data\\apps\\preview-ff\\');
    expect(completed().path).toContain('.\\data\\downloads\\preview-dl1\\');
    expect(data.downloads.find((item) => item.status === 'deleted')).toMatchObject({ projectId: 'preview-kazumi', path: null });
  });
});
