import type { GithubTab, WorkspacePage } from './types';

export const workspacePages = [
  { id: 'home', label: '首页', icon: 'home' },
  { id: 'library', label: '软件库', icon: 'library' },
  { id: 'updates', label: '可更新', icon: 'updates' },
  { id: 'downloads', label: '下载', icon: 'download' },
  { id: 'github', label: 'GitHub', icon: 'github' },
  { id: 'agent', label: 'AI 连接', icon: 'agent' },
] as const;
export const defaultNavigationOrder: WorkspacePage[] = workspacePages.map((page) => page.id);
export const githubTabs: { id: GithubTab; label: string }[] = [
  { id: 'explore', label: '搜索仓库' }, { id: 'owned', label: '我的仓库' },
  { id: 'starred', label: 'Stars 收藏' }, { id: 'activity', label: '关注动态' },
  { id: 'feed', label: '首页动态' },
];

export function normalizeNavigation(order: readonly string[] = []): WorkspacePage[] {
  const known = new Set<string>(defaultNavigationOrder);
  return [...new Set([...order.filter((page) => known.has(page)), ...defaultNavigationOrder])] as WorkspacePage[];
}

export function moveNavigation(order: readonly WorkspacePage[], from: WorkspacePage, to: WorkspacePage): WorkspacePage[] {
  const next = normalizeNavigation(order);
  const start = next.indexOf(from);
  const end = next.indexOf(to);
  if (start < 0 || end < 0 || start === end) return next;
  next.splice(start, 1);
  next.splice(end, 0, from);
  return next;
}
