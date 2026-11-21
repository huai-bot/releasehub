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
  const clients = [
    { id: 'generic', label: '通用 JSON', field: 'genericConfig', hint: '在平台中新建本机 stdio 连接，将 command 和 args 填入对应位置。' },
    { id: 'claude', label: 'Claude', field: 'claudeConfig', hint: '合并到 Claude Code 的 .mcp.json，或 Claude 桌面客户端的 MCP 配置。' },
    { id: 'codex', label: 'Codex', field: 'codexConfig', hint: '将这一段加入 Codex 的 config.toml，保留原有设置。' },
    { id: 'cursor', label: 'Cursor', field: 'cursorConfig', hint: '合并到 Cursor 的 MCP 配置，保留已有的 mcpServers 条目。' },
    { id: 'vscode', label: 'VS Code', field: 'vscodeConfig', hint: '合并到 VS Code 的 MCP 配置，放在 servers 下。' },
  ] as const;
  let client = $state<(typeof clients)[number]['id']>('generic');
  let example = $state(false);
  let portError = $state('');
  let requestId = 0;
  let disposed = false;
  const selectedClient = $derived(clients.find((item) => item.id === client) ?? clients[0]);
  const config = $derived(integration?.[selectedClient.field] ?? '');
  const cliExample = $derived(integration?.cliCommand ? `${integration.cliCommand} list --json` : '');
  async function loadIntegration(asExample = false) {
    const request = ++requestId;
    integration = null; error = ''; loading = true;
    try {
      const result = asExample && preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewAgentIntegration()
        : preview ? null : await onintegration();
      if (disposed || request !== requestId || (!runtime.agent.enabled && !asExample)) return;
      if (!result) throw new Error('没有读到连接配置，请确认开关已开启后重试。');
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
      requestId += 1; integration = null; error = ''; portError = ''; example = false; loading = false;
      if (enabled && !preview) void loadIntegration();
    });
  });
  function savePort(event: SubmitEvent) {
    event.preventDefault();
    if (busy || preview) return;
    const port = Number(new FormData(event.currentTarget as HTMLFormElement).get('port'));
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      portError = '请输入 1024 到 65535 之间的整数端口。';
      return;
    }
    portError = ''; onsave({ agentPort: port });
  }
  onDestroy(() => { disposed = true; requestId += 1; });
</script>

<section class="surface settings-section ai-connection" aria-labelledby="ai-connection-title">
  <div class="settings-heading"><h2 id="ai-connection-title">本机 AI 连接</h2><span class={`status-badge ${runtime.agent.enabled ? 'success' : 'neutral'}`}>{runtime.agent.enabled ? '连接已开启' : settings.agentEnabled ? '连接未启动' : '尚未开启'}</span></div>
  <div class="setting-row"><div><strong>开启软件资料连接</strong><p>只读 · 仅本机 {runtime.agent.address ?? `127.0.0.1:${settings.agentPort}`} · 提供版本、发行包与诊断，不含执行或安装</p></div><Toggle checked={settings.agentEnabled} label="开启软件资料连接" disabled={busy || preview} onchange={() => onsave({ agentEnabled: !settings.agentEnabled })} /></div>
  {#if runtime.agent.error}<p class="field-error" role="alert">{runtime.agent.error}</p>{/if}
  {#if preview}<div class="preview-connection"><span class="field-hint">界面预览 · 虚拟路径示例</span><button class="button secondary small" disabled={loading} onclick={() => { example = true; void loadIntegration(true); }}>查看配置示例</button></div>{/if}
  {#if loading}<div class="config-loading" role="status">正在准备连接配置…<div class="skeleton-lines"><span></span><span></span></div></div>
  {:else if error}<div class="field-error" role="alert">{error}<button class="text-button" onclick={() => void loadIntegration(example)}>重新读取</button></div>
  {:else if integration}
    <div class="config-section">
      <div class="config-toolbar"><div class="segmented-tabs" role="group" aria-label="MCP 平台配置">{#each clients as item}<button class:active={client === item.id} aria-pressed={client === item.id} onclick={() => client = item.id}>{item.label}</button>{/each}</div><button class="button primary small" disabled={preview || !config} onclick={() => oncopy(config)}><Icon name="copy" size={15} />复制配置</button></div>
      <label for="mcp-client-config">{selectedClient.label} · {client === 'codex' ? 'TOML' : 'JSON'}{example ? ' · 示例' : ''}</label>
      <textarea id="mcp-client-config" class="mono" readonly value={config} rows={client === 'codex' ? 4 : 7} spellcheck="false"></textarea>
      <p class="field-hint">{selectedClient.hint}</p>
    </div>
    {#if cliExample}
      <details class="connection-detail"><summary>命令行查询</summary><div class="cli-command"><label for="agent-cli-command" class="field-hint">PowerShell · list / schema</label><textarea id="agent-cli-command" class="mono" readonly value={cliExample} rows={3} spellcheck="false"></textarea><button class="button secondary small" disabled={preview} onclick={() => oncopy(cliExample)}><Icon name="copy" size={15} />复制查询命令</button></div></details>
    {/if}
    {#if integration.tools?.length}<details class="connection-detail"><summary>可用查询工具 · {integration.tools.length}</summary><ul class="tool-list">{#each integration.tools as tool}<li><code>{tool.name}</code><p>{tool.description}</p></li>{/each}</ul></details>{/if}
  {/if}
  <details class="connection-advanced"><summary>本机端口</summary><form class="setting-row last" onsubmit={savePort}><label for="ai-agent-port">连接端口</label><div class="port-input"><input id="ai-agent-port" name="port" type="number" min="1024" max="65535" step="1" required value={settings.agentPort} aria-describedby={portError ? 'agent-port-error' : undefined} aria-invalid={portError ? 'true' : undefined} disabled={busy || preview} /><button class="button secondary small" type="submit" disabled={busy || preview}>应用</button></div></form>{#if portError}<p id="agent-port-error" class="field-error" role="alert">{portError}</p>{/if}{#if integration}<dl class="launch-details"><dt>启动程序</dt><dd><code>{integration.command}</code></dd><dt>启动参数</dt><dd><code>{JSON.stringify(integration.args)}</code></dd></dl>{/if}</details>
</section>

<style>
  .ai-connection { min-width: 0; }
  .preview-connection { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; padding: 11px 0; }
  .config-loading { color: var(--muted); font-size: 12px; margin: 18px 0; }
  .config-section { border-top: 1px solid var(--border); padding-top: 14px; margin-top: 18px; }
  .config-toolbar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px; margin-bottom: 12px; }
  .config-toolbar .segmented-tabs { flex-wrap: wrap; }
  .config-section > label { display: block; color: var(--subtle); font-size: 12px; margin-bottom: 7px; }
  .config-section textarea, .cli-command textarea { width: 100%; font-size: 12px; line-height: 1.6; tab-size: 2; background: var(--surface-soft); }
  .config-section .field-hint { margin-top: 6px; }
  .field-error .text-button { margin-left: 12px; }
  .connection-advanced { border-top: 1px solid var(--border); margin-top: 12px; padding-top: 13px; }
  summary { cursor: pointer; font-size: 12px; color: var(--subtle); padding: 3px 0; }
  summary:hover { color: var(--accent); }
  .connection-detail { margin-top: 14px; }
  .cli-command { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .cli-command label { width: 100%; }
  .tool-list { list-style: none; padding: 0; margin: 10px 0; }
  .tool-list li { padding: 8px 0; border-bottom: 1px solid var(--border); }
  .tool-list code { color: var(--heading); font-size: 12px; overflow-wrap: anywhere; }
  .tool-list p { color: var(--subtle); font-size: 12px; margin-top: 2px; }
  .launch-details { font-size: 12px; margin: 16px 0 0; }
  .launch-details dt { color: var(--muted); margin-top: 8px; }
  .launch-details dd { margin: 3px 0 0; color: var(--subtle); overflow-wrap: anywhere; }
</style>
