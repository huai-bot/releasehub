<script lang="ts">
  import type { AgentIntegration, RuntimeInfo, Settings } from '../lib/types';
  import AiConnection from '../components/AiConnection.svelte';
  import Icon from '../components/Icon.svelte';
  let { settings, runtime, busy, preview, onsave, onintegration, oncopy }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void; onintegration: () => Promise<AgentIntegration | null>; oncopy: (text: string) => void;
  } = $props();
</script>

<div class="page-heading"><div><h1>AI 连接</h1><p>让你常用的 AI 了解软件近况，不把任务执行交给追新。</p></div><span class="quiet-label"><Icon name="shieldCheck" size={16} />仅本机 · 只读</span></div>
<div class="settings-layout"><AiConnection {settings} {runtime} {busy} {preview} {onsave} {onintegration} {oncopy} /></div>
