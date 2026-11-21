<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import type { BackupRecord } from '../lib/types';
  import { formatBytes, formatDate } from '../lib/ui';
  import Icon from './Icon.svelte';
  let { projectId, busy, preview = false, onrestore, ondelete }: {
    projectId: string; busy: boolean; preview?: boolean;
    onrestore: (backup: BackupRecord) => void; ondelete: (backup: BackupRecord) => void;
  } = $props();
  let backup = $state<BackupRecord | null>(null);
  let error = $state('');
  let loading = $state(true);
  let requestId = 0;
  async function load() {
    const request = ++requestId; loading = true; error = '';
    try { const value = preview ? null : await dispatch('get_backup', { projectId }); if (request === requestId) backup = value; }
    catch (cause) { if (request === requestId) error = String(cause); }
    finally { if (request === requestId) loading = false; }
  }
  onMount(() => { void load(); return () => { requestId += 1; }; });
</script>

<section class="backup-panel" aria-label="最近一次备份">
  <header><h3>最近一次备份</h3><button class="icon-button" disabled={busy || loading} aria-label="刷新备份信息" onclick={() => void load()}><Icon name="refresh" size={15} /></button></header>
  {#if loading}<p role="status">正在读取备份记录…</p>
  {:else if error}<p class="field-error" role="alert">{error}</p>
  {:else if backup}
    <p><strong>{backup.version ?? '更新前文件状态'}</strong> · {formatBytes(backup.bytes)}</p><p>{formatDate(backup.createdAt, true)}</p>
    {#if backup.kind === 'installer'}<p>仅文件备份，不包括注册表、服务或安装器其他变更。</p>{/if}
    {#if backup.phase !== 'ready'}<p class="warning-text">{backup.phase === 'recoveryRequired' ? '有未完成恢复，请关闭软件后重试。此备份暂时不能删除。' : '正在安装或恢复，备份暂时不能删除。'}</p>{/if}
    {#if backup.error}<p class="field-error">{backup.error}</p>{/if}
    <div class="backup-actions"><button class="button secondary small" disabled={busy || !['ready', 'recoveryRequired'].includes(backup.phase)} onclick={() => onrestore(backup!)}><Icon name="refresh" size={14} />恢复此备份</button><button class="text-button danger-text" disabled={busy || backup.phase !== 'ready'} onclick={() => ondelete(backup!)}><Icon name="trash" size={14} />删除备份</button></div>
  {:else}<p>{preview ? '预览不读取本机备份。' : '尚无备份。'}</p>{/if}
  {#if backup}<details><summary>备份位置与说明</summary><p class="backup-path">{backup.path}</p><p>只保留最近一次备份。恢复会覆盖当前目录；删除后无法使用此回退点。</p></details>{/if}
</section>
<style>
  .backup-panel { margin-block: 18px; padding-block: 14px; border-block: 1px solid var(--border); }
  header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 10px; }
  h3 { font-size: 13px; font-weight: 650; } p { font-size: 11.5px; line-height: 1.7; color: var(--muted); margin: 5px 0; }
  strong { color: var(--heading); } .backup-path { overflow-wrap: anywhere; }
  .backup-actions { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; margin-block: 12px; }
</style>
