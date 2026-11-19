import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const base = 'http://127.0.0.1:1421';
const output = resolve('.artifacts/ui', process.env.RELEASEHUB_VISUAL_ROUND || 'cleanup-round-1');
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const failures = [], checks = [];
try {
 for (const viewport of [{ width: 960, height: 640 }, { width: 1280, height: 840 }]) {
  const page = await browser.newPage({ viewport, reducedMotion: 'reduce' });
  page.setDefaultTimeout(8000);
  page.on('pageerror', error => failures.push(error.message));
  await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
  const nav = async label => page.locator('.sidebar .nav-item').filter({ has: page.locator('span', { hasText: new RegExp(`^${label}$`) }) }).click();
  const shot = async name => {
   const state = await page.evaluate(() => {
    const modal = document.querySelector('dialog[open]');
    const bounds = modal?.getBoundingClientRect();
    return { overflow: document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight, modal: bounds ? { left: bounds.left, right: bounds.right, top: bounds.top, bottom: bounds.bottom } : null };
   });
   if (state.overflow) failures.push(`${viewport.width}/${name}: document overflow`);
   if (state.modal && (state.modal.left < 0 || state.modal.right > viewport.width + 1 || state.modal.top < 0 || state.modal.bottom > viewport.height + 1)) failures.push(`${viewport.width}/${name}: modal outside viewport`);
   checks.push({ viewport, name, ...state });
   await page.screenshot({ path: resolve(output, `${viewport.width}-${name}.png`) });
  };
  for (const theme of ['light', 'dark']) {
   await page.goto(base); await page.getByRole('button', { name: '进入界面预览', exact: true }).click();
   await nav('设置'); await page.getByRole('button', { name: theme === 'light' ? '浅色 明亮浅灰' : '深色 深色板岩', exact: true }).click();
   await nav('资源库');
   const card = page.locator('.project-card').filter({ hasText: 'FFmpegFreeUI' });
   await card.locator('.card-identity').press('Enter'); await shot(`detail-${theme}`);
   await page.locator('.detail-panel').getByRole('button', { name: '修改本机版本', exact: true }).click();
   if (await page.locator('dialog details').getAttribute('open') !== null) failures.push('Version paths should start collapsed');
   await shot(`version-${theme}`); await page.locator('dialog').getByRole('button', { name: '取消', exact: true }).click();
   await page.locator('.detail-panel').getByRole('button', { name: '项目设置', exact: true }).click(); await shot(`settings-${theme}`);
   await page.locator('dialog').getByRole('button', { name: '完成', exact: true }).click();
   await page.locator('.detail-panel').getByRole('button', { name: '关闭资源详情', exact: true }).click();
   await card.locator('.card-more').click(); await page.getByRole('menuitem', { name: '管理备份', exact: true }).click();
   await shot(`backup-${theme}`); await page.locator('dialog').getByRole('button', { name: '关闭', exact: true }).click();
   await nav('下载'); await shot(`downloads-${theme}`);
   await page.locator('.download-item details summary').first().click(); await shot(`download-more-${theme}`);
  }
  await page.close();
 }
} finally { await browser.close(); }
await writeFile(resolve(output, 'checks.json'), JSON.stringify({ checks, failures }, null, 2));
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Verified ${checks.length} changed-surface renders at two sizes and both themes: ${output}`);
