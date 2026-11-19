import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const base = 'http://127.0.0.1:1421';
const output = resolve('.artifacts/ui', process.env.RELEASEHUB_VISUAL_ROUND || 'installation-round-1');
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const failures = [], checks = [];
try {
 for (const viewport of [{ width: 960, height: 640 }, { width: 1280, height: 840 }]) {
  const page = await browser.newPage({ viewport, reducedMotion: 'reduce' });
  page.setDefaultTimeout(8000); page.on('pageerror', error => failures.push(error.message));
  await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
  await page.goto(base);
  await page.evaluate(async () => {
   const { mount, unmount, flushSync } = await import('/node_modules/svelte/src/index-client.js');
   const { createPreviewSnapshot } = await import('/src/lib/preview.ts');
   const { default: Downloads } = await import('/src/pages/Downloads.svelte');
   const { default: ProjectDetail } = await import('/src/components/ProjectDetail.svelte');
   document.querySelector('#app').style.display = 'none';
   const host = document.createElement('main'); host.className = 'page-content'; host.style.cssText = 'position:fixed;inset:0;overflow:auto;background:var(--bg);padding:24px'; document.body.append(host);
   let instance;
   window.renderInstallation = async (status, theme, detail) => {
    if (instance) await unmount(instance);
    host.replaceChildren();
    document.documentElement.dataset.theme = theme; document.documentElement.dataset.colorScheme = 'blue'; document.documentElement.style.colorScheme = theme;
    const snapshot = createPreviewSnapshot(), project = snapshot.projects[0], download = { ...snapshot.downloads[0], kind: 'installer', status: 'completed', tag: 'v6.5.0' };
    const check = { id: 'synthetic-attempt', projectId: project.id, downloadId: download.id, expectedVersion: 'v6.5.0', previousVersion: project.installedVersion, status, startedAt: '2026-11-19T12:00:00Z', checkedAt: '2026-11-19T12:01:00Z', detectedVersion: status === 'verified' ? '6.5.0' : null, source: status === 'verified' ? 'registry' : null, message: status === 'rebootRequired' ? '安装器要求重启电脑，重启后再验证实际版本。' : status === 'verified' ? 'Windows 安装记录与目标版本一致，已同步本机版本。' : '尚未找到可信的新版本记录，原版本保持不变。完成向导后可立即检测，或手动标记本次版本。' };
    const noop = () => {};
    instance = mount(detail ? ProjectDetail : Downloads, { target: host, props: detail ? { project, installations: [check], busy: false, onVerify: noop, onConfirmInstallation: noop, onclose: noop, oncheck: noop, ondownload: noop, onupdate: noop, onmark: noop, onremove: noop, onopen: noop, onversions: noop, ondiagnostic: noop, onrun: noop, onexecutable: noop, onrestore: noop, ondeletebackup: noop, onSettings: noop } : { downloads: [download], installations: [check], query: '', busy: false, onVerify: noop, onConfirmInstallation: noop, oninstall: noop, onretry: noop, oncancel: noop, onfolder: noop, onmark: noop, onproject: noop, oncopy: noop, onversions: noop } });
    flushSync();
   };
  });
  for (const theme of ['light', 'dark']) for (const status of ['waiting', 'unconfirmed', 'rebootRequired', 'verified']) {
   await page.evaluate(async ({ status, theme }) => window.renderInstallation(status, theme, false), { status, theme });
   const name = `${viewport.width}-${status}-${theme}`;
   const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth || [...document.querySelectorAll('.download-item')].some(node => node.scrollWidth > node.clientWidth + 1));
   if (overflow) failures.push(`${name}: horizontal overflow`);
   checks.push({ name, overflow }); await page.screenshot({ path: resolve(output, `${name}.png`) });
   if (status === 'unconfirmed') { await page.locator('.download-item details summary').click(); await page.screenshot({ path: resolve(output, `${name}-details.png`) }); }
   if (status === 'verified' || status === 'unconfirmed') {
    await page.evaluate(async ({ status, theme }) => window.renderInstallation(status, theme, true), { status, theme });
    await page.screenshot({ path: resolve(output, `${name}-project.png`) });
   }
  }
  await page.close();
 }
} finally { await browser.close(); }
await writeFile(resolve(output, 'checks.json'), JSON.stringify({ checks, failures }, null, 2));
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Verified ${checks.length} installation states plus expanded/project renders: ${output}`);
