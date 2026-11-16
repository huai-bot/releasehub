import { chromium } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

// Full application rendering with the application's explicitly marked preview data.
const base = process.env.RELEASEHUB_VISUAL_URL || 'http://127.0.0.1:1421';
const output = resolve('.artifacts/ui/preferences');
const browser = await chromium.launch({ headless: true, ...(process.env.RELEASEHUB_BROWSER ? { executablePath: process.env.RELEASEHUB_BROWSER } : {}) });
const failures = [];
let startAt = process.env.RELEASEHUB_VISUAL_START || '';
await mkdir(output, { recursive: true });
try {
  for (const viewport of [{ width: 1280, height: 840 }, { width: 960, height: 640 }]) {
    const page = await browser.newPage({ viewport, colorScheme: 'light', reducedMotion: 'reduce' });
    page.on('pageerror', error => failures.push(error.message));
    await page.route('**/*', route => new URL(route.request().url()).origin === new URL(base).origin ? route.continue() : route.abort());
    await page.goto(base);
    await page.getByRole('button', { name: '进入界面预览', exact: true }).click();
    await page.locator('.app-shell').waitFor();
    const check = async name => {
      if (startAt && startAt !== `${viewport.width}-${name}`) return;
      startAt = '';
      const issue = await page.evaluate(() => {
        if (document.documentElement.scrollWidth > document.documentElement.clientWidth) return 'page overflow';
        const sidebar = document.querySelector('.sidebar');
        const controls = document.querySelector('.window-controls');
        if (sidebar && sidebar.getBoundingClientRect().bottom > innerHeight + 1) return 'sidebar overflow';
        if (controls && controls.getBoundingClientRect().right > innerWidth + 1) return 'window controls overflow';
        return null;
      });
      if (issue) failures.push(`${viewport.width}/${name}: ${issue}`);
      await page.screenshot({ path: resolve(output, `${viewport.width}-${name}.png`) });
    };
    const nav = async label => page.locator('.sidebar .nav-item').filter({ has: page.locator('span', { hasText: new RegExp(`^${label}$`) }) }).click();
    await check('home-light');
    await nav('资源库');
    await check('library-light');
    await page.getByRole('button', { name: '文件', exact: true }).click();
    await page.getByRole('button', { name: '查看 文件资料（界面示例） 的版本与说明', exact: true }).click();
    await check('files-detail-light');
    await page.getByRole('complementary').getByRole('button', { name: '关闭资源详情', exact: true }).click();
    await nav('下载');
    await check('downloads-light');
    await nav('设置');
    await page.getByRole('heading', { name: '启动与导航', exact: true }).waitFor();
    await check('settings-light');
    await page.locator('#proxy-mode').selectOption('manual');
    await page.locator('#proxy-url').fill('http://127.0.0.1:7890');
    await page.locator('section[aria-labelledby="proxy-title"]').scrollIntoViewIfNeeded();
    await check('proxy-light');
    await page.getByRole('button', { name: '深色 深色板岩', exact: true }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.locator('#main-content').evaluate(element => element.scrollTop = 0);
    await check('settings-dark');
    for (const [label, scheme] of [['青绿', 'teal'], ['柔紫', 'violet'], ['暖琥珀', 'amber']]) {
      await page.getByRole('button', { name: label, exact: true }).click();
      await page.waitForFunction(value => document.documentElement.dataset.colorScheme === value, scheme);
      await check(`settings-${scheme}-dark`);
    }
    await page.getByRole('button', { name: '经典蓝', exact: true }).click();
    await nav('AI 连接');
    await page.getByRole('button', { name: '查看配置示例', exact: true }).click();
    await page.locator('#mcp-client-config').waitFor();
    await check('mcp-dark');
    await nav('GitHub');
    await page.getByRole('tab', { name: '首页动态', exact: true }).click();
    await page.locator('.feed-list').waitFor();
    await check('feed-dark');
    await page.getByRole('combobox', { name: '首页动态示例状态', exact: true }).selectOption('error');
    await check('feed-error-dark');
    await page.getByRole('combobox', { name: '首页动态示例状态', exact: true }).selectOption('login');
    await check('feed-login-dark');
    await nav('设置');
    await page.getByRole('button', { name: '浅色 明亮浅灰', exact: true }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
    await page.getByLabel('默认打开页面', { exact: true }).selectOption('github');
    await page.getByLabel('GitHub 默认栏目', { exact: true }).selectOption('feed');
    const before = await page.locator('.sidebar nav .nav-item > span:first-of-type').allTextContents();
    const github = page.locator('.sidebar nav .nav-item').filter({ has: page.locator('span', { hasText: /^GitHub$/ }) });
    await github.focus(); await github.press('Alt+ArrowUp');
    const after = await page.locator('.sidebar nav .nav-item > span:first-of-type').allTextContents();
    if (before.join() === after.join() || after.indexOf('GitHub') !== before.indexOf('GitHub') - 1) failures.push(`${viewport.width}: sidebar keyboard order`);
    await nav('首页');
    await check('home-reordered-light');
    await page.close();
  }
} finally { await browser.close(); }
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Application preview verified at 1280×840 and 960×640; screenshots: ${output}`);
