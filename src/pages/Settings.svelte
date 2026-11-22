<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import { formatBytes, readableError } from '../lib/ui';
  import type { AgentIntegration, GithubCacheStats, GithubTab, ProxySettings as ProxyConfig, ProxyTestResult, RuntimeInfo, Settings, Theme, WorkspacePage } from '../lib/types';
  import { defaultNavigationOrder, githubTabs, workspacePages } from '../lib/navigation';
  import Icon from '../components/Icon.svelte';
  import BrandMark from '../components/BrandMark.svelte';
  import Toggle from '../components/Toggle.svelte';
  import AiConnection from '../components/AiConnection.svelte';
  import ProxySettings from '../components/ProxySettings.svelte';
  import ExternalDownloadSettings from '../components/ExternalDownloadSettings.svelte';
  let { settings, runtime, busy, preview, onsave, onproxy, onproxytest, onintegration, ongithub, ondata, onexport, onimport, ontextimport, oncopyexport, oncopy, onopen }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => Promise<boolean>; onintegration: () => Promise<AgentIntegration | null>;
    onproxy: (proxy: ProxyConfig, password?: string) => Promise<boolean>; onproxytest: (proxy: ProxyConfig, password?: string) => Promise<ProxyTestResult>;
    ongithub: () => void; ondata: () => void; onexport: () => void; onimport: () => void; ontextimport: () => void;
    oncopyexport: () => void; oncopy: (text: string) => void; onopen: (url: string) => void;
  } = $props();
  const themes: { id: Theme; title: string; icon: 'sun' | 'moon' | 'monitor' }[] = [
    { id: 'light', title: '浅色', icon: 'sun' },
    { id: 'dark', title: '深色', icon: 'moon' },
    { id: 'system', title: '跟随系统', icon: 'monitor' },
  ];
  let cache = $state<GithubCacheStats | null>(null);
  let cacheBusy = $state(false);
  let cacheMessage = $state('');
  let cacheError = $state(false);
  onMount(() => {
    if (preview) { cache = { bytes: 0, entries: 0 }; return; }
    let active = true;
    void dispatch('github_cache_stats', {}).then(value => { if (active) cache = value; }).catch(() => {});
    return () => { active = false; };
  });
  async function clearCache() {
    if (cacheBusy) return;
    cacheBusy = true; cacheError = false; cacheMessage = '';
    try {
      if (!preview) await dispatch('github_cache_clear', {});
      cache = { bytes: 0, entries: 0 };
      window.dispatchEvent(new Event('releasehub:cache-cleared'));
      cacheMessage = preview ? '预览：页面缓存已清理' : '页面缓存已清理，下载文件和订阅保持不变。';
    } catch (error) { cacheError = true; cacheMessage = readableError(error); }
    finally { cacheBusy = false; }
  }
</script>

<div class="page-heading"><h1>设置</h1></div>
<div class="settings-layout">
  <section class="surface settings-section appearance-section" aria-labelledby="appearance-title">
    <div class="settings-heading"><h2 id="appearance-title">外观</h2></div>
    <div class="theme-options">{#each themes as theme}<button class="theme-option" class:chosen={settings.theme === theme.id} aria-pressed={settings.theme === theme.id} disabled={busy} onclick={() => onsave({ theme: theme.id })}><div class={`theme-preview ${theme.id}`}><span class="preview-sidebar"></span><span class="preview-content"><i></i><i></i><i></i></span>{#if settings.theme === theme.id}<span class="theme-check"><Icon name="check" size={12} /></span>{/if}</div><span><Icon name={theme.icon} size={15} /><strong>{theme.title}</strong></span></button>{/each}</div>

    <p class="field-hint appearance-note">0.6 采用暖米白新拟物界面，强调色统一为钴蓝。</p>
  </section>

  <section class="surface settings-section" aria-labelledby="startup-title"><div class="settings-heading"><h2 id="startup-title">启动与导航</h2></div>
    <div class="setting-row"><label for="startup-page">默认打开页面</label><select id="startup-page" value={settings.startupPage} disabled={busy} onchange={(event) => onsave({ startupPage: event.currentTarget.value as WorkspacePage })}>{#each workspacePages as page}<option value={page.id}>{page.label}</option>{/each}</select></div>
    <div class="setting-row"><label for="startup-github-tab">GitHub 默认栏目</label><select id="startup-github-tab" value={settings.startupGithubTab} disabled={busy} onchange={(event) => onsave({ startupGithubTab: event.currentTarget.value as GithubTab })}>{#each githubTabs as tab}<option value={tab.id}>{tab.label}</option>{/each}</select></div>
    <div class="setting-row last"><strong>工作空间顺序</strong><button class="button secondary small" disabled={busy || settings.navigationOrder.join() === defaultNavigationOrder.join()} onclick={() => onsave({ navigationOrder: [...defaultNavigationOrder] })}><Icon name="retry" size={14} />恢复默认顺序</button></div>
  </section>

  <ProxySettings settings={settings.proxy} passwordConfigured={runtime.proxyPasswordConfigured} {busy} {preview} onsave={onproxy} ontest={onproxytest} />

  <section class="surface settings-section" aria-labelledby="updates-title"><div class="settings-heading"><h2 id="updates-title">更新与下载</h2></div>
    <div class="setting-row"><label for="check-interval">自动检查间隔</label><select id="check-interval" value={settings.checkIntervalMinutes} disabled={busy} onchange={(event) => onsave({ checkIntervalMinutes: Number(event.currentTarget.value) })}>{#if ![0, 15, 30, 60, 180, 360, 1440].includes(settings.checkIntervalMinutes)}<option value={settings.checkIntervalMinutes}>每 {settings.checkIntervalMinutes} 分钟</option>{/if}<option value="0">仅手动检查</option><option value="15">每 15 分钟</option><option value="30">每 30 分钟</option><option value="60">每 1 小时</option><option value="180">每 3 小时</option><option value="360">每 6 小时</option><option value="1440">每天</option></select></div>
    <div class="setting-row"><strong>桌面通知</strong><Toggle checked={settings.notifications} label="桌面通知" disabled={busy} onchange={() => onsave({ notifications: !settings.notifications })} /></div>
    <div class="setting-row"><strong>自动下载选定文件</strong><Toggle checked={settings.autoDownload} label="自动下载选定文件" disabled={busy} onchange={() => onsave({ autoDownload: !settings.autoDownload })} /></div>
    <div class="setting-row"><label for="download-limit">同时下载数量</label><select id="download-limit" value={settings.maxConcurrentDownloads} disabled={busy} onchange={(event) => onsave({ maxConcurrentDownloads: Number(event.currentTarget.value) })}>{#each [1, 2, 3, 4] as count}<option value={count}>{count} 个任务</option>{/each}</select></div>
    <div class="setting-row"><label for="download-connections">单文件连接数</label><select id="download-connections" value={settings.downloadConnections ?? 4} disabled={busy} onchange={(event) => onsave({ downloadConnections: Number(event.currentTarget.value) })}>{#each [1, 2, 3, 4, 5, 6, 7, 8] as count}<option value={count}>{count} 个连接</option>{/each}</select></div>
    <div class="setting-row last"><strong>关闭窗口时收进托盘</strong><Toggle checked={settings.closeToTray} label="关闭窗口时收进托盘" disabled={busy} onchange={() => onsave({ closeToTray: !settings.closeToTray })} /></div>
  </section>

  <ExternalDownloadSettings {settings} {busy} {preview} {onsave} />

  <section class="surface settings-section" aria-labelledby="cache-title">
    <div class="settings-heading"><h2 id="cache-title">页面缓存</h2><span class="quiet-label">≤ 100 MB · 30 天 · 仅本机</span></div>
    <div class="setting-row last"><strong>{cache ? `${formatBytes(cache.bytes)} · ${cache.entries} 条记录` : '本机页面缓存'}</strong><button class="button secondary small" disabled={busy || cacheBusy} onclick={() => void clearCache()}><Icon name="trash" size={15} />{cacheBusy ? '正在清理' : '清理页面缓存'}</button></div>
    {#if cacheMessage}<p class:field-error={cacheError} class:field-hint={!cacheError} role={cacheError ? 'alert' : 'status'}>{cacheMessage}</p>{/if}
  </section>

  <section class="surface settings-section" aria-labelledby="github-title">
    <div class="setting-row last"><h2 id="github-title" class="inline-section-title">GitHub 账号</h2><button class="button secondary small" onclick={ongithub}>管理 GitHub 账号<Icon name="arrowRight" size={15} /></button></div>
  </section>

  <AiConnection {settings} {runtime} {busy} {preview} {onsave} {onintegration} {oncopy} />

  <section class="surface settings-section" aria-labelledby="data-title"><div class="settings-heading"><h2 id="data-title">数据与便携</h2><span class="status-badge neutral">{runtime.portable ? '便携模式' : '安装模式'}</span></div><div class="data-location"><Icon name="folder" size={18} /><code>{runtime.dataDir}</code><button class="button secondary small" disabled={busy} onclick={ondata}>打开目录</button></div><div class="data-actions"><button class="button secondary small" disabled={busy} onclick={onexport}><Icon name="export" size={15} />导出订阅与备注</button><button class="button secondary small" disabled={busy} onclick={onimport}><Icon name="import" size={15} />导入资料</button><button class="text-button" disabled={busy} onclick={ontextimport}>粘贴清单</button><button class="text-button" disabled={busy} onclick={oncopyexport}>复制清单</button></div><p class="field-hint data-note">仅迁移订阅、备注与偏好；账号凭据不迁移，换机后需重新连接。</p></section>
  <section class="about-section"><BrandMark size={30} /><div><strong>ReleaseHub</strong><p>v{runtime.appVersion} · MIT · Windows x64</p></div><button class="text-button" onclick={() => onopen('https://github.com/huai-bot/releasehub')}>项目主页<Icon name="external" size={14} /></button></section>
</div>

<style>
  .appearance-group { margin-top: 20px; padding-top: 18px; border-top: 1px solid var(--border); }
  .appearance-subheading { font-size: 13px; font-weight: 600; color: var(--subtle); margin-bottom: 12px; }
  .inline-section-title { font-size: 15px; font-weight: 650; }
  .data-note { margin-top: 12px; }
  .appearance-note { margin-top: 16px; }
</style>
