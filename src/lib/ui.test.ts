import { describe, expect, it } from 'vitest';
import { downloadPercent, formatBytes, matchesProject, parseRepository, readableError, safeGithubUrl } from './ui';
import { createPreviewSnapshot } from './preview';

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
