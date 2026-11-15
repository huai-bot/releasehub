import type { GithubFeedPage } from '../types';

export type FeedPreviewState = 'connected' | 'login' | 'loading' | 'empty' | 'error';

/** Synthetic UI examples only. They are never a fallback for a failed live read. */
export function previewFeed(state: FeedPreviewState = 'connected'): GithubFeedPage {
  const connected = state !== 'login';
  return {
    connected, loading: state === 'loading', account: connected ? 'preview-account' : null,
    items: state === 'connected' ? [
      { id: 'example-release', actor: '示例开发者', avatarUrl: null, summary: '示例开发者发布了新的桌面工具版本', body: '这是界面示例。真实连接后，这里会按 GitHub 官网实际加载的顺序显示动态文字。', url: 'https://github.com/tauri-apps/tauri/releases', repository: 'tauri-apps/tauri', occurredAt: '2026-11-15T07:20:00Z' },
      { id: 'example-star', actor: '示例关注者', avatarUrl: null, summary: '示例关注者 Star 了 sveltejs/svelte', body: '从动态打开仓库原文，或把感兴趣的软件加入软件库。', url: 'https://github.com/sveltejs/svelte', repository: 'sveltejs/svelte', occurredAt: '2026-11-15T06:40:00Z' },
      { id: 'example-recommendation', actor: 'GitHub', avatarUrl: null, summary: '示例推荐 · 为你发现的新项目', body: '官网的推荐与关注动态会一同保留，不重新排序，也不替换成 Events 或 RSS。', url: 'https://github.com/microsoft/PowerToys', repository: 'microsoft/PowerToys', occurredAt: null },
    ] : [],
    hasMore: state === 'connected', lastUpdated: state === 'connected' || state === 'empty' ? '2026-11-15T07:30:00Z' : null,
    error: state === 'error' ? '示例错误：官网还没有加载出动态，请打开官网窗口检查后重试。' : null,
    sourceUrl: 'https://github.com/dashboard-feed', requestId: `preview-${state}`, sessionRevision: 1,
  };
}
