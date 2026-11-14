import { describe, expect, it } from 'vitest';
import { canActivateManagedVersion, downloadPercent, formatBytes, matchesProject, parseRepository, readableError, safeGithubUrl } from './ui';
import { createPreviewManagedVersions, createPreviewSnapshot } from './preview';

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

describe('readable list states', () => {
  it('searches all words across names and repository identity', () => {
    const project = createPreviewSnapshot().projects[0];
    expect(matchesProject(project, 'lake1059 ffmpeg')).toBe(true);
    expect(matchesProject(project, 'kazumi')).toBe(false);
    expect(matchesProject(project, '  ')).toBe(true);
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
