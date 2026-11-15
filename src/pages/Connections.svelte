<script lang="ts">
  import type { AgentIntegration, RuntimeInfo, Settings } from '../lib/types';
  import AiConnection from '../components/AiConnection.svelte';
  import Icon from '../components/Icon.svelte';
  let { settings, runtime, busy, preview, onsave, onintegration, oncopy }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void; onintegration: () => Promise<AgentIntegration | null>; oncopy: (text: string) => void;
  } = $props();
</script>

<div class="page-heading"><div><h1>AI 连接</h1><p>把版本、发行包和诊断资料交给你的 Agent。</p></div><span class="quiet-label"><Icon name="agent" size={16} />MCP · 命令行</span></div>
<div class="settings-layout"><AiConnection {settings} {runtime} {busy} {preview} {onsave} {onintegration} {oncopy} /></div>
