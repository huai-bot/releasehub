import type { InstallationCheck, InstallationStatus } from './types';

const labels: Record<InstallationStatus, string> = {
  waiting: '等待安装完成', detecting: '正在检测安装结果', verified: '安装版本已同步',
  manual: '已手动记录安装版本', rebootRequired: '等待重启后验证', unconfirmed: '暂未确认安装结果',
  cancelled: '安装已取消', failed: '安装未完成', superseded: '已由后续操作替代',
};
export const installationLabel = (check: InstallationCheck): string => labels[check.status];
export const installationSourceLabel = (source: string | null): string =>
  source === 'registry' ? 'Windows 安装记录' : source === 'executable' ? '主程序版本' : source === 'manual' ? '手动记录' : '待验证';
export const canVerifyInstallation = (check: InstallationCheck): boolean =>
  ['waiting', 'detecting', 'unconfirmed', 'rebootRequired'].includes(check.status);
export const canConfirmInstallation = (check: InstallationCheck): boolean =>
  ['waiting', 'detecting', 'unconfirmed', 'rebootRequired'].includes(check.status);
export function latestInstallation(checks: InstallationCheck[], key: 'projectId' | 'downloadId', id: string): InstallationCheck | null {
  return checks.filter(check => check[key] === id && check.status !== 'superseded')
    .slice().sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0] ?? null;
}
