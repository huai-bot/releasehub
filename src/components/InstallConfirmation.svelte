<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import type { DownloadRecord, InstallPreview } from '../lib/types';
  import { formatBytes } from '../lib/ui';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  let { download, preview = false, busy, onclose, onconfirm }: {
    download: DownloadRecord; preview?: boolean; busy: boolean; onclose: () => void;
    onconfirm: (plan: InstallPreview, archiveRoot?: string) => Promise<void>;
  } = $props();
  let target = $state('');
  let plan = $state<InstallPreview | null>(null);
  let archiveRoot = $state('');
  let confirmed = $state(false);
  let loading = $state(false);
  let error = $state('');
  let requestId = 0;
  const ready = $derived(plan !== null && target.trim() === plan.targetPath && (plan.kind !== 'portable' || plan.archiveRoots.length === 0 || plan.archiveRoots.includes(archiveRoot)));
  async function inspect() {
    confirmed = false; plan = null; error = ''; loading = true;
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
      if (value) { target = value; await inspect(); }
    } catch (cause) { error = String(cause); }
  }
  onMount(() => { void inspect(); return () => { requestId += 1; }; });
</script>

<Modal title={download.kind === 'portable' ? '确认更新到软件原目录' : '备份后打开官方安装器'} description="先核对目标与备份。只有确认后才会写入软件文件。" {onclose} wide>
  <form class="modal-form" onsubmit={(event) => { event.preventDefault(); if (ready && confirmed && plan) void onconfirm(plan, archiveRoot || undefined); }}>
    <div class="confirm-package"><Icon name="package" size={28} /><div><strong>{download.projectName} <span>{download.tag}</span></strong><p>{download.fileName}</p><small>{formatBytes(download.totalBytes)}</small></div></div>
    <label>软件原目录<div class="input-with-button"><input data-initial-focus value={target} oninput={(event) => { target = event.currentTarget.value; confirmed = false; }} placeholder="选择已安装软件的目录；首装请选择专用空目录" maxlength="1000" disabled={busy || loading} /><button type="button" class="button secondary" disabled={busy || loading || preview} onclick={() => void browse()}>浏览</button></div></label>
    <button class="button secondary small" type="button" disabled={busy || loading || preview} onclick={() => void inspect()}><Icon name={loading ? 'loader' : 'shield'} class={loading ? 'spinning' : ''} size={15} />{loading ? '正在检查目录与安装包' : '核查目标和备份空间'}</button>
    {#if plan}
      <dl class="install-facts"><div><dt>版本</dt><dd>{plan.previousVersion ?? '未记录'} → {plan.nextVersion}</dd></div><div><dt>备份位置</dt><dd>{plan.backupPath}</dd></div><div><dt>完整备份</dt><dd>{formatBytes(plan.backupBytes)} · 每个软件仅保留最近一次</dd></div><div><dt>可用空间</dt><dd>{formatBytes(plan.availableBytes)}</dd></div></dl>
      {#if plan.kind === 'portable' && plan.archiveRoots.length}
        <label>安装包的程序目录<select bind:value={archiveRoot} disabled={busy} onchange={() => confirmed = false}><option value="">请选择要写入的软件目录</option>{#each plan.archiveRoots as root}<option value={root}>{root === '.' ? '安装包根目录（保持原结构）' : root}</option>{/each}</select></label>
      {/if}
      <div class="confirmation-details"><p>先完整备份，再用新版替换同名文件；安装包没有的原文件继续保留。同名配置文件也会被新版替换。</p><p>本次更新成功后才替换上一回退点。恢复会撤回更新后的配置改动；详情中的“最近一次备份”可恢复或删除。</p>{#if plan.legacyManaged}<p class="warning-text">当前使用旧受管位置。迁移必须明确选择新的软件专用空目录；成功后才切换记录，旧位置不会自动删除。</p>{/if}{#if plan.kind === 'installer'}<p class="warning-text">EXE / MSI 只创建文件备份，再打开作者安装向导。请在向导中选择上述原目录。文件恢复不能完整回退注册表、服务等变更，完成后需手动确认版本。</p>{/if}<p class="integrity-disclaimer"><Icon name="shield" size={15} />{download.verified ? '已匹配上游文件摘要，仍请核对软件来源。' : '上游没有可比对的摘要，请核对项目来源。'}</p></div>
      <label class="confirmation-check"><input type="checkbox" bind:checked={confirmed} required disabled={!ready || busy} />我已核对目录、版本、覆盖规则，并同意备份后继续。</label>
    {/if}
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
    <div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={onclose}>暂不安装</button><button class="button primary" type="submit" disabled={busy || loading || !ready || !confirmed}><Icon name={busy ? 'loader' : 'install'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在备份并处理' : download.kind === 'portable' ? '确认备份并更新' : '确认备份并打开向导'}</button></div>
  </form>
</Modal>

<style>
  .install-facts { margin: 18px 0; border-block: 1px solid var(--border); padding: 12px 0; }
  .install-facts > div { display: grid; grid-template-columns: 82px minmax(0, 1fr); gap: 12px; padding: 5px 0; font-size: 12px; line-height: 1.6; }
  dt { color: var(--muted); } dd { margin: 0; overflow-wrap: anywhere; color: var(--heading); font-variant-numeric: tabular-nums; }
</style>
