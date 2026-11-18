import { chromium } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

// This harness serves synthetic account data locally. It never calls GitHub or the desktop service.
const base = process.env.RELEASEHUB_VISUAL_URL || 'http://127.0.0.1:1421';
const output = resolve('.artifacts/ui/github-lists');
const browser = await chromium.launch({ headless: true, ...(process.env.RELEASEHUB_BROWSER ? { executablePath: process.env.RELEASEHUB_BROWSER } : {}) });
const failures = [];
const states = new Set((process.env.RELEASEHUB_VISUAL_STATES || '').split(',').filter(Boolean));
await mkdir(output, { recursive: true });
const mockBridge = `
import { createPreviewGithubDetail, createPreviewGithubStatus, createPreviewGithubListRepositories, createPreviewGithubLists, createPreviewRepositoryLists, createPreviewRepositoryNotes } from '/src/lib/github-preview.ts';
export const desktopAvailable = () => false;
export const watchChanges = async () => () => {};
let notes = createPreviewRepositoryNotes();
export async function dispatch(action, payload = {}) {
  if (action === 'feed_status') return { connected: false, loading: false, sessionRevision: 0, account: null, items: [], error: null };
  if (action === 'github_status') return createPreviewGithubStatus('connected');
  if (action === 'github_lists') return createPreviewGithubLists('connected');
  if (action === 'github_list_repositories') return createPreviewGithubListRepositories(payload.listId, payload.cursor, 'connected');
  if (action === 'github_repository') return createPreviewGithubDetail(payload.repository, true);
  if (action === 'github_repository_lists') return { listIds: createPreviewRepositoryLists(payload.repository) };
  if (action === 'get_repository_notes') return notes;
  if (action === 'save_repository_note') {
    notes = notes.filter(note => note.repository.toLowerCase() !== payload.repository.toLowerCase());
    if (!payload.name && !payload.description) return null;
    const result = { ...payload, updatedAt: '2026-01-01T00:00:00Z' }; notes.push(result); return result;
  }
  throw new Error('Visual fixture does not implement ' + action);
}
`;
const html = `<!doctype html><html lang="zh-CN" data-theme="light"><head><meta charset="utf-8"><title>GitHub Lists synthetic visual check</title><link rel="stylesheet" href="/src/app.css"></head><body><div style="display:flex;height:100vh;overflow:hidden"><aside class="sidebar" aria-hidden="true" style="min-height:100vh">ReleaseHub · 追新</aside><main id="fixture" class="page-content github-workspace" style="min-width:0"></main></div><script type="module">
import { mount } from '/node_modules/.vite/deps/svelte.js';
import Github from '/src/pages/Github.svelte';
mount(Github, { target: document.getElementById('fixture'), props: { projects: [], preview: false, initialTab: 'starred', onadd: async () => true, onopen: () => {}, oncopy: () => {}, onnotice: () => {} } });
</script></body></html>`;
try {
  for (const viewport of [{ width: 1280, height: 840 }, { width: 960, height: 640 }, { width: 1920, height: 1260 }]) {
    const page = await browser.newPage({ viewport, colorScheme: 'light', reducedMotion: 'reduce' });
    page.on('pageerror', error => failures.push(error.message));
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.origin !== new URL(base).origin) return route.abort();
      if (url.pathname === '/__github_lists_visual__') return route.fulfill({ contentType: 'text/html', body: html });
      if (url.pathname === '/src/lib/bridge.ts') return route.fulfill({ contentType: 'application/javascript', body: mockBridge });
      return route.continue();
    });
    await page.goto(`${base}/__github_lists_visual__`);
    await page.getByRole('button', { name: '查看 Lake1059/FFmpegFreeUI 详情', exact: true }).waitFor();
    const check = async name => {
      if (states.size && !states.has(name)) return;
      const layout = await page.evaluate(() => ({
        overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
        clippedActions: [...document.querySelectorAll('.github-row-actions button')].filter(button => {
          if (!button.getClientRects().length) return false;
          const rect = button.getBoundingClientRect(); const row = button.closest('.repo-row').getBoundingClientRect();
          return rect.left < row.left || rect.right > row.right || rect.top < row.top || rect.bottom > row.bottom;
        }).map(button => button.getAttribute('aria-label') || button.textContent.trim()),
        scrolledPanels: [...document.querySelectorAll('.github-results')].some(panel => panel.scrollLeft !== 0),
      }));
      if (layout.overflow || layout.clippedActions.length || layout.scrolledPanels) failures.push(`${name}: bad layout at ${viewport.width}: ${JSON.stringify(layout)}`);
      await page.screenshot({ path: resolve(output, `${viewport.width}-${name}.png`), fullPage: true });
    };
    await check('stars-light');
    await page.getByRole('button', { name: '设置 Lake1059/FFmpegFreeUI 分类', exact: true }).click();
    await page.getByRole('button', { name: '确认同步分类', exact: true }).waitFor({ state: 'visible' });
    await page.waitForFunction(() => ![...document.querySelectorAll('button')].find(button => button.textContent.trim() === '确认同步分类')?.disabled);
    await check('assignment-light');
    await page.getByRole('button', { name: '关闭分类整理', exact: true }).click();
    await page.getByRole('button', { name: '新建分类', exact: true }).click();
    await page.locator('.list-editor input').first().scrollIntoViewIfNeeded();
    await check('new-list-light');
    await page.locator('.list-editor').getByRole('button', { name: '取消', exact: true }).click();
    await page.getByRole('button', { name: '查看 Lake1059/FFmpegFreeUI 详情', exact: true }).click();
    await page.locator('.github-detail .repo-heading h2').waitFor();
    await check('detail-light');
    await page.getByRole('button', { name: '编辑本地备注', exact: true }).click();
    await page.getByRole('dialog').waitFor();
    await check('note-light');
    await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click();
    await page.getByRole('button', { name: '返回列表', exact: true }).click();
    await page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; document.documentElement.style.colorScheme = 'dark'; });
    await check('stars-dark');
    await page.close();
  }
} finally { await browser.close(); }
if (failures.length) throw new Error(failures.join('\n'));
console.log(`GitHub Lists visual checks passed at 960×640, 1280×840 and 1920×1260; screenshots: ${output}`);
