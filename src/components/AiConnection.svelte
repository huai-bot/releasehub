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
  <div class="settings-heading"><Icon name="agent" size={19} /><div><h2 id="ai-connection-title">连接你的 Agent 平台</h2><p>通过 MCP 或命令行，把软件资料交给你常用的 AI。</p></div><span class={`status-badge ${runtime.agent.enabled ? 'success' : 'neutral'}`}>{runtime.agent.enabled ? '连接已开启' : settings.agentEnabled ? '连接未启动' : '尚未开启'}</span></div>
  <div class="setting-row"><div><strong>开启软件资料连接</strong><p>使用期间保持追新打开，凭据由本机连接程序自动获取。</p></div><Toggle checked={settings.agentEnabled} label="开启软件资料连接" disabled={busy || preview} onchange={() => onsave({ agentEnabled: !settings.agentEnabled })} /></div>
  {#if runtime.agent.error}<p class="field-error" role="alert">{runtime.agent.error}</p>{/if}
  <dl class="capability-list">
    <div><dt>版本与发行包</dt><dd>软件列表、版本、安装位置和发行文件</dd></div>
    <div><dt>源码与诊断</dt><dd>固定源码提交、下载错误和基础环境</dd></div>
  </dl>
  <p class="handoff-note">追新提供资料；编译、修复和安装由你的外部 Agent 平台完成。</p>
  <ol class="connection-steps"><li>开启连接，选择平台格式并复制配置，保留平台已有的连接。</li><li>在同一台电脑的客户端添加并重新载入连接，然后让 Agent 读取软件列表。</li></ol>
  <p class="field-hint">支持能启动本机 stdio 程序的 MCP 客户端；纯云端连接无法直接访问这台电脑。</p>
  {#if preview}<div class="preview-connection"><span class="field-hint">当前为界面预览，可查看虚拟路径示例。</span><button class="button secondary small" disabled={loading} onclick={() => { example = true; void loadIntegration(true); }}>查看配置示例</button></div>{/if}
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
      <details class="connection-detail"><summary>命令行查询</summary><p class="field-hint">在 PowerShell 粘贴下方命令。把 list 换成 schema 可看全部用法；去掉 --json 则显示易读文字。</p><div class="cli-command"><label for="agent-cli-command" class="field-hint">查询已保存的软件列表</label><textarea id="agent-cli-command" class="mono" readonly value={cliExample} rows={3} spellcheck="false"></textarea><button class="button secondary small" disabled={preview} onclick={() => oncopy(cliExample)}><Icon name="copy" size={15} />复制查询命令</button></div></details>
    {/if}
    {#if integration.tools?.length}<details class="connection-detail"><summary>可用查询工具 · {integration.tools.length} 项</summary><ul class="tool-list">{#each integration.tools as tool}<li><code>{tool.name}</code><p>{tool.description}</p></li>{/each}</ul></details>{/if}
  {:else if !runtime.agent.enabled}<p class="closed-hint"><Icon name="disconnected" size={16} />开启后会显示这台电脑的配置和查询命令。</p>{/if}
  <details class="connection-advanced"><summary>高级设置 · 本机端口</summary><form class="setting-row last" onsubmit={savePort}><div><label for="ai-agent-port">连接端口</label><p>{runtime.agent.address ?? `127.0.0.1:${settings.agentPort}`} · 通常无需修改</p></div><div class="port-input"><input id="ai-agent-port" name="port" type="number" min="1024" max="65535" step="1" required value={settings.agentPort} aria-describedby={portError ? 'agent-port-error' : undefined} aria-invalid={portError ? 'true' : undefined} disabled={busy || preview} /><button class="button secondary small" type="submit" disabled={busy || preview}>应用</button></div></form>{#if portError}<p id="agent-port-error" class="field-error" role="alert">{portError}</p>{/if}{#if integration}<dl class="launch-details"><dt>启动程序</dt><dd><code>{integration.command}</code></dd><dt>启动参数</dt><dd><code>{JSON.stringify(integration.args)}</code></dd></dl>{/if}</details>
</section>

<style>
  .ai-connection { min-width: 0; }
  .capability-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px 20px; margin: 16px 0 8px; }
  .capability-list dt { font-size: 12px; color: var(--heading); font-weight: 600; }
  .capability-list dd { margin: 3px 0 0; font-size: 12px; color: var(--subtle); }
  .handoff-note { font-size: 12px; color: var(--subtle); }
  .connection-steps { margin: 16px 0 6px; padding-left: 20px; color: var(--subtle); font-size: 12px; line-height: 1.7; }
  .connection-steps li { padding-left: 4px; margin: 4px 0; }
  .connection-steps li::marker { color: var(--muted); font-variant-numeric: tabular-nums; }
  .preview-connection { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; padding: 11px 0; }
  .config-loading { color: var(--muted); font-size: 12px; margin: 18px 0; }
  .config-section { border-top: 1px solid var(--border); padding-top: 14px; margin-top: 18px; }
  .config-toolbar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px; margin-bottom: 12px; }
  .config-toolbar .segmented-tabs { flex-wrap: wrap; }
  .config-section > label { display: block; color: var(--subtle); font-size: 12px; margin-bottom: 7px; }
  .config-section textarea, .cli-command textarea { width: 100%; font-size: 12px; line-height: 1.6; tab-size: 2; background: var(--surface-soft); }
  .config-section .field-hint { margin-top: 6px; }
  .field-error .text-button { margin-left: 12px; }
  .closed-hint { display: flex; align-items: center; gap: 8px; padding: 13px 0; color: var(--muted); font-size: 12px; }
  .connection-advanced { border-top: 1px solid var(--border); margin-top: 16px; padding-top: 13px; }
  summary { cursor: pointer; font-size: 12px; color: var(--subtle); padding: 3px 0; }
  summary:hover { color: var(--accent); }
  .connection-detail { margin-top: 14px; }
  .connection-detail > .field-hint { margin: 8px 0; }
  .cli-command { display: flex; flex-wrap: wrap; gap: 8px; }
  .cli-command label { width: 100%; }
  .tool-list { list-style: none; padding: 0; margin: 10px 0; }
  .tool-list li { padding: 8px 0; border-bottom: 1px solid var(--border); }
  .tool-list code { color: var(--heading); font-size: 12px; overflow-wrap: anywhere; }
  .tool-list p { color: var(--subtle); font-size: 12px; margin-top: 2px; }
  .launch-details { font-size: 12px; margin: 16px 0 0; }
  .launch-details dt { color: var(--muted); margin-top: 8px; }
  .launch-details dd { margin: 3px 0 0; color: var(--subtle); overflow-wrap: anywhere; }
</style>
