// Development preview only. These are invented examples, not official listings or downloadable files.
import type { TrainerCatalogMode, TrainerDetail, TrainerEntry, TrainerListPage } from './trainer-catalog';

const titles = ['远山回声', '星港远航', '石桥以北', '纸上航线', '群岛来信', '微光车站'];
const examples: TrainerEntry[] = titles.map((title, i) => ({ title, url: `https://flingtrainer.com/trainer/releasehub-example-${i}/`,
  publishedAt: null, updatedAt: `2026-11-${String(26 - i * 2).padStart(2, '0')}T10:00:00Z`, summary: '界面预览使用的游戏条目，不代表官网实际收录。' }));

export function previewList(mode: TrainerCatalogMode, query: string): TrainerListPage {
  const items = examples.filter(e => mode !== 'search' || e.title.includes(query));
  if (mode === 'all') items.sort((a, b) => a.title.localeCompare(b.title, 'zh-CN'));
  return { items, page: 1, hasNextPage: false, totalPages: items.length ? 1 : 0 };
}
export function previewDetail(url: string): TrainerDetail {
  const entry = examples.find(item => item.url === url);
  if (!entry) throw new Error('示例条目不存在。');
  return { ...entry, content: '这里会显示官网提供的游戏版本、使用说明和更新记录。',
    attachments: [{ label: `${entry.title} · 示例附件`, url: `${entry.url}example` }] };
}
