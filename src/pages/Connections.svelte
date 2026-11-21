<script lang="ts">
  import type { AgentIntegration, RuntimeInfo, Settings } from '../lib/types';
  import AiConnection from '../components/AiConnection.svelte';
  let { settings, runtime, busy, preview, onsave, onintegration, oncopy }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void; onintegration: () => Promise<AgentIntegration | null>; oncopy: (text: string) => void;
  } = $props();
</script>

<div class="page-heading"><h1>AI 连接</h1></div>
<div class="settings-layout"><AiConnection {settings} {runtime} {busy} {preview} {onsave} {onintegration} {oncopy} /></div>
