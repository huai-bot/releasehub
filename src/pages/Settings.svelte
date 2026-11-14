<script lang="ts">
  import { onMount } from 'svelte';
  import type { AgentConnection, RuntimeInfo, Settings, Theme } from '../lib/types';
  import Icon from '../components/Icon.svelte';
  import Toggle from '../components/Toggle.svelte';
  let { settings, runtime, busy, onsave, onsettoken, onconnection, ondata, onexport, onimport, ontextimport, oncopyexport, oncopy, onopen }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean;
    onsave: (changes: Partial<Settings>) => void; onsettoken: (token: string) => Promise<void>;
    onconnection: (rotate: boolean) => Promise<AgentConnection | null>;
    ondata: () => void; onexport: () => void; onimport: () => void; ontextimport: () => void;
    oncopyexport: () => void; oncopy: (text: string) => void; onopen: (url: string) => void;
  } = $props();
  let token = $state('');
  let connection = $state<AgentConnection | null>(null);
  let rotateConfirm = $state(false);
  let secretTimer: ReturnType<typeof setTimeout> | undefined;
  let revealId = 0;
  let disposed = false;
  const themes: { id: Theme; title: string; icon: 'sun' | 'moon' | 'monitor'; description: string }[] = [
    { id: 'light', title: '浅色', icon: 'sun', description: '温润纸白' },
    { id: 'dark', title: '深色', icon: 'moon', description: '安静夜绿' },
    { id: 'system', title: '跟随系统', icon: 'monitor', description: '与 Windows 一致' },
  ];
  function hideConnection() { revealId += 1; connection = null; if (secretTimer) clearTimeout(secretTimer); secretTimer = undefined; }
  function clearSecrets() { hideConnection(); token = ''; rotateConfirm = false; }
  async function reveal(rotate = false) {
    hideConnection(); rotateConfirm = false;
    const requestId = revealId;
    const result = await onconnection(rotate);
    // A late response must not restore a secret after hiding or leaving the window.
    if (disposed || requestId !== revealId || document.hidden || !runtime.agent.enabled || !result) return;
    connection = result;
    secretTimer = setTimeout(hideConnection, 60_000);
  }
  async function saveToken(event: SubmitEvent) {
    event.preventDefault(); const next = token.trim(); token = '';
    if (next) await onsettoken(next);
  }
  $effect(() => { if (!runtime.agent.enabled && connection) hideConnection(); });
  onMount(() => {
    const visibility = () => { if (document.hidden) clearSecrets(); };
    document.addEventListener('visibilitychange', visibility);
    window.addEventListener('blur', clearSecrets);
    window.addEventListener('pagehide', clearSecrets);
    return () => {
      disposed = true; clearSecrets();
      document.removeEventListener('visibilitychange', visibility);
      window.removeEventListener('blur', clearSecrets);
      window.removeEventListener('pagehide', clearSecrets);
    };
  });
</script>

<div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>设置</h1><p>按你的习惯工作。设置跟着软件库走，密钥留在这台电脑。</p></div><span class="quiet-label"><Icon name="settings" size={16} />更改后自动保存</span></div>
<div class="settings-layout">
  <section class="surface settings-section" aria-labelledby="appearance-title"><div class="settings-heading"><Icon name="sun" size={19} /><div><h2 id="appearance-title">外观</h2><p>选择舒适的工作空间</p></div></div><div class="theme-options">{#each themes as theme}<button class="theme-option" class:chosen={settings.theme === theme.id} aria-pressed={settings.theme === theme.id} disabled={busy} onclick={() => onsave({ theme: theme.id })}><div class={`theme-preview ${theme.id}`}><span class="preview-sidebar"></span><span class="preview-content"><i></i><i></i><i></i></span>{#if settings.theme === theme.id}<span class="theme-check"><Icon name="check" size={12} /></span>{/if}</div><span><Icon name={theme.icon} size={15} /><strong>{theme.title}</strong></span><small>{theme.description}</small></button>{/each}</div></section>

  <section class="surface settings-section" aria-labelledby="updates-title"><div class="settings-heading"><Icon name="refresh" size={19} /><div><h2 id="updates-title">更新与下载</h2><p>自动留意动态，安装仍由你决定</p></div></div>
    <div class="setting-row"><div><label for="check-interval">自动检查间隔</label><p>运行期间按此间隔检查已订阅软件</p></div><select id="check-interval" value={settings.checkIntervalMinutes} disabled={busy} onchange={(event) => onsave({ checkIntervalMinutes: Number(event.currentTarget.value) })}>{#if ![0, 15, 30, 60, 180, 360, 1440].includes(settings.checkIntervalMinutes)}<option value={settings.checkIntervalMinutes}>每 {settings.checkIntervalMinutes} 分钟</option>{/if}<option value="0">仅手动检查</option><option value="15">每 15 分钟</option><option value="30">每 30 分钟</option><option value="60">每 1 小时</option><option value="180">每 3 小时</option><option value="360">每 6 小时</option><option value="1440">每天</option></select></div>
    <div class="setting-row"><div><strong>桌面通知</strong><p>发现新发行版时提醒我</p></div><Toggle checked={settings.notifications} label="桌面通知" disabled={busy} onchange={() => onsave({ notifications: !settings.notifications })} /></div>
    <div class="setting-row"><div><strong>自动下载安装包</strong><p>只下载确定匹配的文件，有歧义时等待手选；绝不自动安装</p></div><Toggle checked={settings.autoDownload} label="自动下载安装包" disabled={busy} onchange={() => onsave({ autoDownload: !settings.autoDownload })} /></div>
    <div class="setting-row"><div><label for="download-limit">同时下载数量</label><p>限制并行任务，给其他工作留些带宽</p></div><select id="download-limit" value={settings.maxConcurrentDownloads} disabled={busy} onchange={(event) => onsave({ maxConcurrentDownloads: Number(event.currentTarget.value) })}>{#each [1, 2, 3, 4] as count}<option value={count}>{count} 个任务</option>{/each}</select></div>
    <div class="setting-row last"><div><strong>关闭窗口时收进托盘</strong><p>在后台继续留意更新，可从托盘退出</p></div><Toggle checked={settings.closeToTray} label="关闭窗口时收进托盘" disabled={busy} onchange={() => onsave({ closeToTray: !settings.closeToTray })} /></div>
  </section>

  <section class="surface settings-section" aria-labelledby="github-title"><div class="settings-heading"><Icon name="github" size={19} /><div><h2 id="github-title">GitHub 访问</h2><p>公开项目不登录也能使用。凭据是可选的。</p></div><span class={`status-badge ${runtime.githubTokenConfigured ? 'success' : 'neutral'}`}>{runtime.githubTokenConfigured ? '已配置凭据' : '匿名访问'}</span></div>
    <div class="setting-note"><Icon name="shieldCheck" size={17} /><p>凭据交给 Windows 凭据管理器保存，不会出现在订阅导出中，也不会回显已保存的内容。仅访问公开项目时，无需授予仓库写入权限。</p></div>
    <form class="token-form" onsubmit={saveToken}><label for="github-token">{runtime.githubTokenConfigured ? '更换 GitHub Token' : '配置 GitHub Token'}<span class="optional">可选</span></label><div class="input-with-button"><input id="github-token" type="password" bind:value={token} autocomplete="new-password" spellcheck="false" placeholder="粘贴新的 Token，提交后立即清空" maxlength="1024" disabled={busy} /><button class="button secondary" type="submit" disabled={busy || !token.trim()}>安全保存</button></div></form>
    <div class="settings-bottom-line"><span class="field-hint">换到新电脑后，需要重新连接凭据。</span>{#if runtime.githubTokenConfigured}<button class="text-button danger-text" disabled={busy} onclick={() => { token = ''; void onsettoken(''); }}>移除本机凭据</button>{/if}</div>
  </section>

  <section class="surface settings-section" aria-labelledby="agent-settings-title"><div class="settings-heading"><Icon name="agent" size={19} /><div><h2 id="agent-settings-title">本机 Agent 连接</h2><p>让外部 Agent 领取任务与回传构建结果</p></div><span class={`status-badge ${runtime.agent.enabled ? 'success' : 'neutral'}`}>{runtime.agent.enabled ? '已开启' : '默认关闭'}</span></div>
    <div class="setting-row"><div><strong>允许本机 Agent 连接</strong><p>仅监听 127.0.0.1，必须携带专用密钥，不对局域网开放</p></div><Toggle checked={runtime.agent.enabled} label="允许本机 Agent 连接" disabled={busy} onchange={() => { hideConnection(); onsave({ agentEnabled: !runtime.agent.enabled }); }} /></div>
    <form class="setting-row" onsubmit={(event) => { event.preventDefault(); const port = Number(new FormData(event.currentTarget).get('port')); hideConnection(); onsave({ agentPort: port }); }}><div><label for="agent-port">连接端口</label><p>{runtime.agent.address ?? `127.0.0.1:${settings.agentPort}`} · 开启后生效</p></div><div class="port-input"><input id="agent-port" name="port" type="number" min="1024" max="65535" step="1" required value={settings.agentPort} disabled={busy} /><button class="button secondary small" type="submit" disabled={busy}>应用</button></div></form>
    {#if runtime.agent.error}<div class="field-error" role="alert">{runtime.agent.error}</div>{/if}
    <div class="agent-permissions"><span><Icon name="check" size={13} />读取软件元数据</span><span><Icon name="check" size={13} />领取与回传任务</span><span><Icon name="cancel" size={13} />不能执行命令或安装</span></div>
    <div class="connection-actions"><button class="button secondary small" disabled={busy || !runtime.agent.enabled} onclick={() => connection ? hideConnection() : void reveal()}><Icon name={connection ? 'eyeOff' : 'eye'} size={15} />{connection ? '隐藏连接信息' : '查看连接信息'}</button><button class="text-button" disabled={busy || !runtime.agent.enabled} onclick={() => rotateConfirm = !rotateConfirm}><Icon name="retry" size={14} />重新生成密钥</button></div>
    {#if rotateConfirm}<div class="rotation-confirm"><p>重新生成后，已有 Agent 需要更新连接密钥。确定继续吗？</p><div><button class="button secondary small" disabled={busy} onclick={() => void reveal(true)}>确认重新生成</button><button class="text-button" onclick={() => rotateConfirm = false}>取消</button></div></div>{/if}
    {#if connection}<div class="secret-panel"><div><strong>连接信息 · 仅显示 60 秒</strong><button class="icon-button" aria-label="立即隐藏连接信息" onclick={hideConnection}><Icon name="close" size={15} /></button></div><label>接口地址<input readonly value={connection.address} aria-label="Agent 接口地址" /></label><label>专用密钥<div class="input-with-button"><input readonly type="text" value={connection.token} autocomplete="off" spellcheck="false" aria-label="Agent 专用密钥，仅当前显示" /><button class="icon-button" title="复制密钥到剪贴板" aria-label="复制 Agent 密钥到剪贴板" onclick={() => oncopy(connection!.token)}><Icon name="copy" size={15} /></button></div></label><p>仅交给你信任的本机 Agent。离开本页或页面不可见后隐藏，不保存在浏览器中。</p></div>{/if}
    <p class="field-hint">连接信息只在你主动查看时获取。接口不开启网页跨域访问，不自动调用模型或付费服务。</p>
  </section>

  <section class="surface settings-section" aria-labelledby="data-title"><div class="settings-heading"><Icon name="drive" size={19} /><div><h2 id="data-title">数据与便携</h2><p>{runtime.portable ? '当前使用便携模式，数据跟随应用目录' : '当前使用安装模式，数据保存在本机应用目录'}</p></div><span class="status-badge neutral">{runtime.portable ? '便携模式' : '安装模式'}</span></div><div class="data-location"><Icon name="folder" size={18} /><code>{runtime.dataDir}</code><button class="button secondary small" disabled={busy} onclick={ondata}>打开目录</button></div><p class="setting-description">导出只带走订阅和偏好，不含密钥、任务或本机绝对安装路径。完整迁移便携历史与受管软件时，请先退出追新，再复制整个便携文件夹。</p><div class="data-actions"><button class="button secondary small" disabled={busy} onclick={onexport}><Icon name="export" size={15} />导出订阅</button><button class="button secondary small" disabled={busy} onclick={onimport}><Icon name="import" size={15} />导入订阅</button><button class="text-button" disabled={busy} onclick={ontextimport}>粘贴清单</button><button class="text-button" disabled={busy} onclick={oncopyexport}>复制清单</button></div></section>

  <section class="about-section"><span class="brand-mark small"><Icon name="package" size={20} /></span><div><strong>ReleaseHub <span>追新</span></strong><p>v{runtime.appVersion} · MIT 开源 · Windows x64</p></div><button class="text-button" onclick={() => onopen('https://github.com/huai-bot/releasehub')}>项目主页<Icon name="external" size={14} /></button></section>
</div>
