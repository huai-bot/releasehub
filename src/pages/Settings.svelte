<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import { formatBytes, readableError } from '../lib/ui';
  import type { AgentIntegration, ColorScheme, GithubCacheStats, GithubTab, ProxySettings as ProxyConfig, ProxyTestResult, RuntimeInfo, Settings, Theme, WorkspacePage } from '../lib/types';
  import { defaultNavigationOrder, githubTabs, workspacePages } from '../lib/navigation';
  import Icon from '../components/Icon.svelte';
  import BrandMark from '../components/BrandMark.svelte';
  import Toggle from '../components/Toggle.svelte';
  import AiConnection from '../components/AiConnection.svelte';
  import ProxySettings from '../components/ProxySettings.svelte';
  let { settings, runtime, busy, preview, onsave, onproxy, onproxytest, onintegration, ongithub, ondata, onexport, onimport, ontextimport, oncopyexport, oncopy, onopen }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void; onintegration: () => Promise<AgentIntegration | null>;
    onproxy: (proxy: ProxyConfig, password?: string) => Promise<boolean>; onproxytest: (proxy: ProxyConfig, password?: string) => Promise<ProxyTestResult>;
    ongithub: () => void; ondata: () => void; onexport: () => void; onimport: () => void; ontextimport: () => void;
    oncopyexport: () => void; oncopy: (text: string) => void; onopen: (url: string) => void;
  } = $props();
  const themes: { id: Theme; title: string; icon: 'sun' | 'moon' | 'monitor'; description: string }[] = [
    { id: 'light', title: '浅色', icon: 'sun', description: '明亮浅灰' },
    { id: 'dark', title: '深色', icon: 'moon', description: '深色板岩' },
    { id: 'system', title: '跟随系统', icon: 'monitor', description: '与 Windows 一致' },
  ];
  const palettes: { id: ColorScheme; title: string; color: string; soft: string }[] = [
    { id: 'blue', title: '经典蓝', color: '#1d4ed8', soft: '#e8effd' },
    { id: 'teal', title: '青绿', color: '#0f766e', soft: '#e4f3ef' },
    { id: 'violet', title: '柔紫', color: '#6d42bb', soft: '#f1eafb' },
    { id: 'amber', title: '暖琥珀', color: '#965d0b', soft: '#fcf1dc' },
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

<div class="page-heading"><div><h1>设置</h1><p>调整外观、启动页面、网络和更新习惯。</p></div><span class="quiet-label"><Icon name="settings" size={16} />偏好更改后自动保存</span></div>
<div class="settings-layout">
  <section class="surface settings-section" aria-labelledby="appearance-title"><div class="settings-heading"><Icon name="sun" size={19} /><div><h2 id="appearance-title">外观</h2><p>窗口与工作空间使用统一主题</p></div></div><div class="theme-options">{#each themes as theme}<button class="theme-option" class:chosen={settings.theme === theme.id} aria-pressed={settings.theme === theme.id} disabled={busy} onclick={() => onsave({ theme: theme.id })}><div class={`theme-preview ${theme.id}`}><span class="preview-sidebar"></span><span class="preview-content"><i></i><i></i><i></i></span>{#if settings.theme === theme.id}<span class="theme-check"><Icon name="check" size={12} /></span>{/if}</div><span><Icon name={theme.icon} size={15} /><strong>{theme.title}</strong></span><small>{theme.description}</small></button>{/each}</div></section>

  <section class="surface settings-section palette-section" aria-labelledby="palette-title">
    <div class="settings-heading"><Icon name="sun" size={19} /><div><h2 id="palette-title">配色方案</h2><p>搭配任意明暗模式，品牌图标保持经典蓝色</p></div></div>
    <div class="palette-options">{#each palettes as palette}<button class="palette-option" class:chosen={(settings.colorScheme ?? 'blue') === palette.id} aria-pressed={(settings.colorScheme ?? 'blue') === palette.id} disabled={busy} onclick={() => onsave({ colorScheme: palette.id })} style={`--palette-color:${palette.color};--palette-soft:${palette.soft}`}><span class="palette-swatch" aria-hidden="true"><i></i><i></i><i></i></span><strong>{palette.title}</strong>{#if (settings.colorScheme ?? 'blue') === palette.id}<Icon name="check" size={15} />{/if}</button>{/each}</div>
  </section>

  <section class="surface settings-section" aria-labelledby="startup-title"><div class="settings-heading"><Icon name="home" size={19} /><div><h2 id="startup-title">启动与导航</h2><p>打开追新后，直接进入常用页面</p></div></div>
    <div class="setting-row"><div><label for="startup-page">默认打开页面</label><p>下次启动时生效</p></div><select id="startup-page" value={settings.startupPage} disabled={busy} onchange={(event) => onsave({ startupPage: event.currentTarget.value as WorkspacePage })}>{#each workspacePages as page}<option value={page.id}>{page.label}</option>{/each}</select></div>
    <div class="setting-row"><div><label for="startup-github-tab">GitHub 默认栏目</label><p>进入 GitHub 页面时打开的栏目</p></div><select id="startup-github-tab" value={settings.startupGithubTab} disabled={busy} onchange={(event) => onsave({ startupGithubTab: event.currentTarget.value as GithubTab })}>{#each githubTabs as tab}<option value={tab.id}>{tab.label}</option>{/each}</select></div>
    <div class="setting-row last"><div><strong>工作空间顺序</strong><p>拖动左侧页面即可调整，也可选中后按 Alt + 上下方向键</p></div><button class="button secondary small" disabled={busy || settings.navigationOrder.join() === defaultNavigationOrder.join()} onclick={() => onsave({ navigationOrder: [...defaultNavigationOrder] })}><Icon name="retry" size={14} />恢复默认顺序</button></div>
  </section>

  <ProxySettings settings={settings.proxy} passwordConfigured={runtime.proxyPasswordConfigured} {busy} {preview} onsave={onproxy} ontest={onproxytest} />

  <section class="surface settings-section" aria-labelledby="updates-title"><div class="settings-heading"><Icon name="refresh" size={19} /><div><h2 id="updates-title">更新与下载</h2><p>设置检查频率、下载方式与通知</p></div></div>
    <div class="setting-row"><div><label for="check-interval">自动检查间隔</label><p>运行期间按此间隔检查已订阅资源</p></div><select id="check-interval" value={settings.checkIntervalMinutes} disabled={busy} onchange={(event) => onsave({ checkIntervalMinutes: Number(event.currentTarget.value) })}>{#if ![0, 15, 30, 60, 180, 360, 1440].includes(settings.checkIntervalMinutes)}<option value={settings.checkIntervalMinutes}>每 {settings.checkIntervalMinutes} 分钟</option>{/if}<option value="0">仅手动检查</option><option value="15">每 15 分钟</option><option value="30">每 30 分钟</option><option value="60">每 1 小时</option><option value="180">每 3 小时</option><option value="360">每 6 小时</option><option value="1440">每天</option></select></div>
    <div class="setting-row"><div><strong>桌面通知</strong><p>发现新发行版时提醒我</p></div><Toggle checked={settings.notifications} label="桌面通知" disabled={busy} onchange={() => onsave({ notifications: !settings.notifications })} /></div>
    <div class="setting-row"><div><strong>自动下载选定文件</strong><p>发现更新后下载匹配的文件，有多个选择时等待手动选择</p></div><Toggle checked={settings.autoDownload} label="自动下载选定文件" disabled={busy} onchange={() => onsave({ autoDownload: !settings.autoDownload })} /></div>
    <div class="setting-row"><div><label for="download-limit">同时下载数量</label><p>限制并行任务，给其他工作留些带宽</p></div><select id="download-limit" value={settings.maxConcurrentDownloads} disabled={busy} onchange={(event) => onsave({ maxConcurrentDownloads: Number(event.currentTarget.value) })}>{#each [1, 2, 3, 4] as count}<option value={count}>{count} 个任务</option>{/each}</select></div>
    <div class="setting-row last"><div><strong>关闭窗口时收进托盘</strong><p>在后台继续留意更新，可从托盘退出</p></div><Toggle checked={settings.closeToTray} label="关闭窗口时收进托盘" disabled={busy} onchange={() => onsave({ closeToTray: !settings.closeToTray })} /></div>
  </section>

  <section class="surface settings-section" aria-labelledby="cache-title">
    <div class="settings-heading"><Icon name="drive" size={19} /><div><h2 id="cache-title">页面缓存</h2><p>先显示已保存的内容，再在后台检查新变化</p></div></div>
    <div class="setting-row last"><div><strong>{cache ? `${formatBytes(cache.bytes)} · ${cache.entries} 条记录` : '本机页面缓存'}</strong><p>最多 100MB，保留 30 天；不会随便携资料迁移</p></div><button class="button secondary small" disabled={busy || cacheBusy} onclick={() => void clearCache()}><Icon name="trash" size={15} />{cacheBusy ? '正在清理' : '清理页面缓存'}</button></div>
    {#if cacheMessage}<p class:field-error={cacheError} class:field-hint={!cacheError} role={cacheError ? 'alert' : 'status'}>{cacheMessage}</p>{/if}
  </section>

  <section class="surface settings-section" aria-labelledby="github-title"><div class="settings-heading"><Icon name="github" size={19} /><div><h2 id="github-title">GitHub 账号</h2><p>浏览仓库，整理 Stars 分类，查看关注与首页动态</p></div></div><div class="settings-bottom-line"><span class="field-hint">通过 GitHub 官方授权连接账号</span><button class="button secondary small" onclick={ongithub}>管理 GitHub 账号<Icon name="arrowRight" size={15} /></button></div></section>

  <AiConnection {settings} {runtime} {busy} {preview} {onsave} {onintegration} {oncopy} />

  <section class="surface settings-section" aria-labelledby="data-title"><div class="settings-heading"><Icon name="drive" size={19} /><div><h2 id="data-title">数据与便携</h2><p>{runtime.portable ? '当前使用便携模式，数据跟随应用目录' : '当前使用安装模式，数据保存在本机应用目录'}</p></div><span class="status-badge neutral">{runtime.portable ? '便携模式' : '安装模式'}</span></div><div class="data-location"><Icon name="folder" size={18} /><code>{runtime.dataDir}</code><button class="button secondary small" disabled={busy} onclick={ondata}>打开目录</button></div><p class="setting-description">导出内容包括订阅、项目备注和使用偏好。完整迁移历史记录与受管软件时，退出追新后复制整个便携文件夹；换电脑后重新连接账号。</p><div class="data-actions"><button class="button secondary small" disabled={busy} onclick={onexport}><Icon name="export" size={15} />导出订阅与备注</button><button class="button secondary small" disabled={busy} onclick={onimport}><Icon name="import" size={15} />导入资料</button><button class="text-button" disabled={busy} onclick={ontextimport}>粘贴清单</button><button class="text-button" disabled={busy} onclick={oncopyexport}>复制清单</button></div></section>
  <section class="about-section"><BrandMark size={30} /><div><strong>ReleaseHub <span>追新</span></strong><p>v{runtime.appVersion} · MIT 开源 · Windows x64</p></div><button class="text-button" onclick={() => onopen('https://github.com/huai-bot/releasehub')}>项目主页<Icon name="external" size={14} /></button></section>
</div>
