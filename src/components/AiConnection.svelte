<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import type { AgentIntegration, RuntimeInfo, Settings } from '../lib/types';
  import { readableError } from '../lib/ui';
  import Icon from './Icon.svelte';
  import Toggle from './Toggle.svelte';
  let { settings, runtime, busy, preview, onsave, onintegration, oncopy }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void; onintegration: () => Promise<AgentIntegration | null>; oncopy: (text: string) => void;
  } = $props();
  let integration = $state<AgentIntegration | null>(null);
  let loading = $state(false);
  let error = $state('');
  let client = $state<'claude' | 'codex'>('claude');
  let example = $state(false);
  let requestId = 0;
  let disposed = false;
  const config = $derived(client === 'claude' ? integration?.claudeConfig ?? '' : integration?.codexConfig ?? '');
  async function loadIntegration(asExample = false) {
    const request = ++requestId;
    integration = null; error = ''; loading = true;
    try {
      const result = asExample && preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewAgentIntegration()
        : preview ? null : await onintegration();
      if (disposed || request !== requestId || (!runtime.agent.enabled && !asExample)) return;
      if (!result || result.readOnly !== true) throw new Error('没有读到只读连接配置，请确认已开启连接后重试。');
      integration = result;
    } catch (cause) { if (!disposed && request === requestId) error = readableError(cause); }
    finally { if (!disposed && request === requestId) loading = false; }
  }
  $effect(() => {
    const enabled = runtime.agent.enabled;
    const port = settings.agentPort;
    const address = runtime.agent.address;
    untrack(() => {
      void port; void address;
      requestId += 1; integration = null; error = ''; example = false; loading = false;
      if (enabled && !preview) void loadIntegration();
    });
  });
  onDestroy(() => { disposed = true; requestId += 1; });
</script>

<section class="surface settings-section ai-connection" aria-labelledby="ai-connection-title">
  <div class="settings-heading"><Icon name="agent" size={19} /><div><h2 id="ai-connection-title">连接 Codex / Claude Code</h2><p>在你自己的 AI 客户端安装 MCP 连接，追新只提供已保存的信息。</p></div><span class={`status-badge ${runtime.agent.enabled ? 'success' : 'neutral'}`}>{runtime.agent.enabled ? '已开启 · 只读' : settings.agentEnabled ? '连接未启动' : '默认关闭'}</span></div>
  <div class="setting-row"><div><strong>允许本机 AI 读取信息</strong><p>只允许同一台电脑连接；使用期间需要保持追新打开。</p></div><Toggle checked={settings.agentEnabled} label="允许本机 AI 读取信息" disabled={busy || preview} onchange={() => onsave({ agentEnabled: !settings.agentEnabled })} /></div>
  {#if runtime.agent.error}<p class="field-error" role="alert">{runtime.agent.error}</p>{/if}
  <div class="permission-summary"><div><Icon name="check" size={16} /><p><strong>可以读取</strong>软件列表、已保存的版本、安装位置和下载记录等信息。</p></div><div><Icon name="shield" size={16} /><p><strong>不会执行</strong>命令、下载或安装，也不会读取 GitHub 凭据或第三方 AI 密钥。</p></div></div>
  <ol class="connection-steps"><li>在同一台电脑安装并打开 <strong>Codex</strong> 或 <strong>Claude Code</strong>。</li><li>开启上方开关，把下方对应的 MCP 配置加入客户端配置，保留已有条目。</li><li>保存后重新载入客户端连接，并保持追新打开。关闭开关即可停止读取。</li></ol>
  {#if preview}<div class="preview-connection"><span class="field-hint">预览不会开启连接，也不会读取本机配置。</span><button class="button secondary small" disabled={loading} onclick={() => { example = true; void loadIntegration(true); }}>查看配置示例</button></div>{/if}
  {#if loading}<div class="config-loading" role="status">正在准备无密钥的连接配置…<div class="skeleton-lines"><span></span><span></span></div></div>
  {:else if error}<div class="field-error" role="alert">{error}<button class="text-button" onclick={() => void loadIntegration(example)}>重新读取</button></div>
  {:else if integration}
    <div class="config-section"><div class="config-toolbar"><div class="segmented-tabs" aria-label="AI 客户端配置"><button class:active={client === 'claude'} aria-pressed={client === 'claude'} onclick={() => client = 'claude'}>Claude Code</button><button class:active={client === 'codex'} aria-pressed={client === 'codex'} onclick={() => client = 'codex'}>Codex</button></div><button class="button secondary small" disabled={preview} onclick={() => oncopy(config)}><Icon name="copy" size={15} />复制配置</button></div><label for="mcp-client-config">{client === 'claude' ? 'Claude Code 的 MCP 配置（JSON）' : 'Codex 的 MCP 配置（TOML）'}{example ? ' · 示例，不可直接使用' : ''}</label><textarea id="mcp-client-config" class="mono" readonly value={config} rows={client === 'claude' ? 11 : 4} spellcheck="false"></textarea><p class="field-hint">配置只有本机启动路径和参数，不含密钥，无需输入、查看或重置密钥。</p></div>
  {:else if !runtime.agent.enabled}<p class="closed-hint"><Icon name="disconnected" size={16} />连接尚未开启。开启后才会生成可复制的本机配置。</p>{/if}
  <details class="connection-advanced"><summary>高级设置 · 本机端口</summary><form class="setting-row last" onsubmit={(event) => { event.preventDefault(); const port = Number(new FormData(event.currentTarget).get('port')); onsave({ agentPort: port }); }}><div><label for="ai-agent-port">连接端口</label><p>{runtime.agent.address ?? `127.0.0.1:${settings.agentPort}`} · 通常无需修改</p></div><div class="port-input"><input id="ai-agent-port" name="port" type="number" min="1024" max="65535" step="1" required value={settings.agentPort} disabled={busy || preview} /><button class="button secondary small" type="submit" disabled={busy || preview}>应用</button></div></form>{#if integration}<dl><dt>启动程序</dt><dd><code>{integration.command}</code></dd><dt>启动参数</dt><dd><code>{JSON.stringify(integration.args)}</code></dd></dl>{/if}</details>
</section>

<style>
  .ai-connection { min-width: 0; }
  .permission-summary { display: flex; flex-direction: column; gap: 11px; margin: 18px 0; }
  .permission-summary > div { display: flex; gap: 8px; align-items: flex-start; color: var(--muted); }
  .permission-summary :global(svg) { color: var(--accent); margin-top: 3px; }
  .permission-summary p { color: var(--subtle); font-size: 12px; line-height: 1.8; }
  .permission-summary strong { margin-right: 8px; color: var(--heading); font-weight: 600; }
  .connection-steps { margin: 19px 0; padding-left: 21px; color: var(--subtle); font-size: 12px; line-height: 1.9; }
  .connection-steps li { padding-left: 4px; margin: 6px 0; }
  .connection-steps li::marker { color: var(--muted); font-variant-numeric: tabular-nums; }
  .preview-connection { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; padding: 11px 0; }
  .config-loading { color: var(--muted); font-size: 12px; margin: 18px 0; }
  .config-section { border-top: 1px solid var(--border); padding-top: 14px; margin-top: 18px; }
  .config-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; }
  .config-section > label { display: block; color: var(--subtle); font-size: 11.5px; margin-bottom: 7px; }
  .config-section textarea { width: 100%; font-size: 11.5px; line-height: 1.7; tab-size: 2; background: var(--surface-soft); }
  .config-section .field-hint { margin-top: 6px; }
  .field-error .text-button { margin-left: 12px; }
  .closed-hint { display: flex; align-items: center; gap: 8px; padding: 13px 0; color: var(--muted); font-size: 12px; }
  .connection-advanced { border-top: 1px solid var(--border); margin-top: 16px; padding-top: 13px; }
  summary { cursor: pointer; font-size: 11.5px; color: var(--muted); }
  summary:hover { color: var(--accent); }
  dl { font-size: 11px; margin: 17px 0 0; }
  dt { color: var(--muted); margin-top: 8px; }
  dd { margin: 3px 0 0; color: var(--subtle); overflow-wrap: anywhere; }
</style>
