<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import type { DownloadRecord, InstallPreview } from '../lib/types';
  import { formatBytes } from '../lib/ui';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  let { download, initialPlan = null, actionError = '', preview = false, busy, onclose, onconfirm }: {
    download: DownloadRecord; initialPlan?: InstallPreview | null; actionError?: string; preview?: boolean; busy: boolean; onclose: () => void;
    onconfirm: (plan: InstallPreview, archiveRoot?: string) => Promise<void>;
  } = $props();
  let target = $state('');
  let plan = $state<InstallPreview | null>(null);
  let archiveRoot = $state('');
  let loading = $state(false);
  let error = $state('');
  let requestId = 0;
  const ready = $derived(plan !== null && target.trim() === plan.targetPath && (!plan.requiresRootSelection || plan.archiveRoots.includes(archiveRoot)));
  async function inspect() {
    plan = null; error = ''; loading = true;
    const request = ++requestId;
    try {
      if (preview) throw new Error('界面预览不会读取或修改本机软件。请在桌面版继续。');
      const value = await dispatch('preview_install', { id: download.id, ...(target.trim() ? { targetPath: target.trim() } : {}) });
      if (request !== requestId) return;
      plan = value; target = value.targetPath; archiveRoot = value.suggestedRoot ?? '';
    } catch (cause) { if (request === requestId) error = String(cause).replace(/^Error: /, ''); }
    finally { if (request === requestId) loading = false; }
  }
  async function browse() {
    error = '';
    try {
      if (preview) return;
      const value = await dispatch('pick_directory', {});
      if (value) { target = value; plan = null; await inspect(); }
    } catch (cause) { error = String(cause); }
  }
  onMount(() => {
    if (initialPlan) { plan = initialPlan; target = initialPlan.targetPath; archiveRoot = initialPlan.suggestedRoot ?? ''; }
    else void inspect();
    return () => { requestId += 1; };
  });
</script>

<Modal title={download.kind === 'portable' ? '选择软件目录' : '打开官方安装器'} description="只补充无法自动确定的信息" {onclose}>
  <form class="modal-form" onsubmit={(event) => { event.preventDefault(); if (ready && plan) void onconfirm(plan, plan.requiresRootSelection ? archiveRoot : plan.suggestedRoot ?? undefined); }}>
    <div class="confirm-package"><Icon name="package" size={25} /><div><strong>{download.projectName} <span>{download.tag}</span></strong><p>{download.fileName}</p><small>{formatBytes(download.totalBytes)}</small></div></div>
    <label>软件目录<div class="input-with-button"><input data-initial-focus value={target} oninput={(event) => { target = event.currentTarget.value; plan = null; }} placeholder="选择安装目录" maxlength="1000" disabled={busy || loading} /><button type="button" class="button secondary" disabled={busy || loading || preview} onclick={() => void browse()}>浏览</button></div></label>
    {#if plan?.requiresRootSelection && plan.archiveRoots.length}
      <label>安装包内的软件目录<select bind:value={archiveRoot} disabled={busy || loading}><option value="">请选择目录</option>{#each plan.archiveRoots as root}<option value={root}>{root === '.' ? '安装包根目录' : root}</option>{/each}</select></label>
    {/if}
    {#if plan?.legacyManaged}<p class="field-hint">此目录已通过迁移检查，将保留旧目录并迁入配置。</p>{/if}
    {#if plan}<p class="field-hint">{plan.backupEnabled ? '更新前会自动备份。' : '此软件未启用自动备份，本次不会创建备份。'}</p>{/if}
    {#if error || actionError}<p class="field-error" role="alert">{error || actionError}</p>{/if}
    <div class="modal-footer">
      {#if !plan || target.trim() !== plan.targetPath}<button type="button" class="button secondary" disabled={busy || loading || preview} onclick={() => void inspect()}>{loading ? '正在检查…' : '检查目录'}</button>{/if}
      <button type="button" class="button secondary" disabled={busy} onclick={onclose}>取消</button>
      <button class="button primary" type="submit" disabled={busy || loading || !ready}><Icon name={busy ? 'loader' : 'install'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理…' : download.kind === 'portable' ? '安装 / 更新' : '打开安装器'}</button>
    </div>
  </form>
</Modal>
