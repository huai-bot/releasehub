import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const base = 'http://127.0.0.1:1421';
const output = resolve('.artifacts/ui', process.env.RELEASEHUB_VISUAL_ROUND || 'round-1');
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const failures = [], checks = [];
try {
 for (const viewport of [{ width: 960, height: 640 }, { width: 1280, height: 840 }, { width: 1920, height: 1260 }]) {
  const page = await browser.newPage({ viewport, reducedMotion: 'reduce' });
  page.on('pageerror', error => failures.push(error.message));
  await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
  await page.goto(base); await page.getByRole('button', { name: '进入界面预览', exact: true }).click();
  const nav = async label => { await page.locator('.sidebar .nav-item').filter({ has: page.locator('span', { hasText: new RegExp(`^${label}$`) }) }).click(); };
  const shot = async name => {
   const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight);
   if (overflow) failures.push(`${viewport.width}/${name}: document overflow`);
   await page.screenshot({ path: resolve(output, `${viewport.width}-${name}.png`) });
  };
  for (const theme of ['light', 'dark']) {
   await nav('设置'); await page.getByRole('button', { name: theme === 'light' ? '浅色 明亮浅灰' : '深色 深色板岩', exact: true }).click();
   await nav('首页'); await shot(`home-${theme}`);
   await nav('资源库'); await shot(`library-${theme}`);
   await page.getByRole('button', { name: '查看 FFmpegFreeUI 详情', exact: true }).click();
   await shot(`library-detail-${theme}`);
   await page.locator('.detail-panel').getByRole('button', { name: '关闭资源详情', exact: true }).click();
   await nav('可更新'); await shot(`updates-${theme}`);
   await nav('下载'); await shot(`downloads-${theme}`);
   await page.getByRole('button', { name: '全部', exact: true }).click();
   await nav('GitHub'); await page.getByRole('tab', { name: '搜索仓库', exact: true }).click();
   await page.locator('.repo-main').first().click(); await page.locator('.github-detail .repo-heading h2').waitFor();
   await shot(`github-detail-${theme}`);
   const dims = await page.evaluate(() => {
    const list = document.querySelector('.github-results'), detail = document.querySelector('.detail-region');
    return { list: list.getBoundingClientRect().toJSON(), detail: detail.getBoundingClientRect().toJSON(), listOverflow: getComputedStyle(list).overflowY, detailOverflow: getComputedStyle(detail).overflowY };
   });
   checks.push({ viewport, theme, dims });
   if (dims.list.height < 90 || dims.detail.height < 90 || dims.list.right > dims.detail.left + 1) failures.push(`${viewport.width}/${theme}: unusable two columns`);
   // Bounded synthetic long-content fixture proves wheel isolation at each supported width.
   await page.evaluate(() => {
    const list = document.querySelector('.github-results'), detail = document.querySelector('.detail-region');
    for (let i = 0; i < 12; i++) list.append(list.querySelector('.repo-row').cloneNode(true));
    for (let i = 0; i < 12; i++) detail.append(detail.querySelector('.detail-body').cloneNode(true));
   });
   const list = page.locator('.github-results'), detail = page.locator('.detail-region');
   await list.hover(); await page.mouse.wheel(0, 350); await page.waitForTimeout(180);
   const one = await page.evaluate(() => ({ list: document.querySelector('.github-results').scrollTop, detail: document.querySelector('.detail-region').scrollTop, parent: document.querySelector('.github-workspace').scrollTop }));
   await detail.hover(); await page.mouse.wheel(0, 350); await page.waitForTimeout(180);
   const two = await page.evaluate(() => ({ list: document.querySelector('.github-results').scrollTop, detail: document.querySelector('.detail-region').scrollTop, parent: document.querySelector('.github-workspace').scrollTop }));
   if (one.list <= 0 || one.detail !== 0 || two.detail <= 0 || two.list !== one.list || one.parent || two.parent) failures.push(`${viewport.width}/${theme}: scroll isolation ${JSON.stringify({ one, two })}`);
   // Recreate preview after the synthetic fixture so screenshots remain genuine.
   await page.reload(); await page.getByRole('button', { name: '进入界面预览', exact: true }).click();
  }
  await page.close();
 }
} finally { await browser.close(); }
await writeFile(resolve(output, 'checks.json'), JSON.stringify({ checks, failures }, null, 2));
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Verified themes, supported window sizes and independent wheel scrolling: ${output}`);
