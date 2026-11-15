<script lang="ts">
  import { tick } from 'svelte';
  import type { ManagedVersion, Project } from '../lib/types';
  import { canActivateManagedVersion } from '../lib/ui';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  let { project, versions, loading, error, actionError, busy, preview, onclose, onrefresh, onopen, onactivate }: {
    project: Project | null; versions: ManagedVersion[]; loading: boolean; error: string;
    actionError: string; busy: boolean; preview: boolean; onclose: () => void; onrefresh: () => void;
    onopen: (downloadId: string) => void; onactivate: (downloadId: string, confirmed: boolean) => Promise<boolean>;
  } = $props();
  let selectedId = $state<string | null>(null);
  let confirmed = $state(false);
  let confirmationInput = $state<HTMLInputElement>();
  let refreshButton = $state<HTMLButtonElement>();
  const selectedVersion = $derived(versions.find((version) => version.downloadId === selectedId));
  const canConfirm = $derived(!preview && !busy && !loading && !error && canActivateManagedVersion(project, selectedVersion, confirmed));

  async function choose(version: ManagedVersion) {
    selectedId = version.downloadId; confirmed = false;
    await tick(); confirmationInput?.focus();
  }
  async function backToList() {
    selectedId = null; confirmed = false;
    await tick(); refreshButton?.focus();
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canConfirm || !selectedVersion) return;
    if (await onactivate(selectedVersion.downloadId, confirmed)) await backToList();
  }
</script>

<Modal title={selectedVersion && !loading && !error ? '确认切换当前版本' : '管理已准备版本'} description={project ? `${project.name} · 管理追新保存的免安装版本` : '软件订阅已不存在'} wide {onclose}>
  <div class="modal-form">
    {#if preview}<div class="setting-note"><Icon name="monitor" size={18} /><p><strong>界面预览：以下版本和目录均为示例。</strong>可查看各版本的切换步骤。</p></div>{/if}
    {#if actionError}<p class="field-error" role="alert">{actionError}</p>{/if}
    {#if !project}
      <div class="empty-state compact"><Icon name="info" size={28} /><h3>这个软件已取消订阅</h3><p>重新订阅后，可继续管理已有版本。</p></div>
    {:else if project.installedSource !== 'managed'}
      <div class="empty-state compact"><Icon name="shield" size={28} /><h3>外部安装的软件不在这里切换</h3><p>当前版本来自手动记录。请在软件详情中更新版本与安装位置。</p></div>
    {:else if loading}
      <div class="empty-state compact" role="status"><Icon name="loader" class="spinning" size={28} /><h3>正在读取已准备版本</h3><p>核对版本记录和仍然存在的程序目录。</p></div>
    {:else if error}
      <div class="empty-state compact"><Icon name="alert" size={28} /><h3>版本列表没有读取成功</h3><p class="managed-error" role="alert">{error}</p></div>
    {:else if selectedVersion}
      <form onsubmit={submit}>
        <div class="confirmation-details">
          <p>当前记录：<strong>{project.installedVersion ?? '尚未记录'}</strong></p>
          <p>准备切换为：<strong>{selectedVersion.version}</strong></p>
          <div class="managed-path"><span>切换后的程序目录</span><code>{selectedVersion.path}</code></div>
          <p><strong>将所选版本设为追新的当前版本。</strong></p>
          <p>各版本使用自己的目录与配置。切换后可打开对应的程序目录。</p>
          {#if selectedVersion.active}<p class="success-text">这个版本已经是当前版本，无需再次切换。</p>{/if}
        </div>
        <label class="confirmation-check"><input bind:this={confirmationInput} type="checkbox" bind:checked={confirmed} required disabled={busy || selectedVersion.active} />我已核对版本与目录，确认设为当前版本。</label>
        <div class="modal-footer"><button class="button secondary" type="button" disabled={busy} onclick={() => void backToList()}>返回版本列表</button><button class="button primary" type="submit" disabled={!canConfirm}><Icon name={busy ? 'loader' : 'check'} class={busy ? 'spinning' : ''} size={16} />{preview ? '预览中不可切换' : busy ? '正在切换记录' : '确认设为当前'}</button></div>
      </form>
    {:else}
      <p class="field-hint managed-intro">这里显示已准备完成的免安装版本，可打开目录或设为当前版本。</p>
      <div class="managed-list">
        {#each versions as version (version.downloadId)}
          <article class="managed-version" class:current-version={version.active}>
            <div class="managed-version-heading"><Icon name="package" size={19} /><strong>{version.version}</strong><span class={`status-badge ${version.active ? 'success' : 'neutral'}`}>{version.active ? '当前版本' : '已准备'}</span></div>
            <div class="managed-path"><span>程序目录</span><code>{version.path}</code></div>
            <div class="managed-version-actions"><button class="button secondary small" disabled={busy || preview} aria-label={`打开 ${version.version} 的程序目录`} title="打开此版本的程序目录" onclick={() => onopen(version.downloadId)}><Icon name="folder" size={15} />打开程序目录</button>{#if !version.active}<button class="button secondary small" disabled={busy} onclick={() => void choose(version)}><Icon name="retry" size={15} />设为当前版本</button>{/if}</div>
          </article>
        {:else}<div class="empty-state compact"><Icon name="package" size={28} /><h3>还没有可用的已准备版本</h3><p>请先在下载页把免安装包安装到受管目录。</p></div>{/each}
      </div>
    {/if}
    {#if !selectedVersion || loading || error || !project || project.installedSource !== 'managed'}
      <div class="modal-footer"><button bind:this={refreshButton} class="button secondary" disabled={busy || loading || !project || project.installedSource !== 'managed'} onclick={onrefresh}><Icon name="refresh" size={15} />{error ? '重新读取' : '刷新版本列表'}</button><button class="button primary" disabled={busy} onclick={onclose}>完成</button></div>
    {/if}
  </div>
</Modal>

<style>
  .managed-intro { margin: 0 0 14px; }
  .managed-list { display: flex; flex-direction: column; gap: 12px; }
  .managed-version { padding: 15px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-soft); }
  .current-version { border-color: var(--border-strong); background: var(--accent-soft); }
  .managed-version-heading { display: flex; align-items: center; gap: 9px; color: var(--accent); }
  .managed-version-heading strong { flex: 1; min-width: 0; overflow-wrap: anywhere; color: var(--heading); font-size: 14px; }
  .managed-path { margin: 12px 0; }
  .managed-path span { display: block; color: var(--muted); font-size: 10px; margin-bottom: 4px; }
  .managed-path code { display: block; font-size: 11px; color: var(--subtle); overflow-wrap: anywhere; user-select: text; }
  .managed-version-actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 8px; margin-top: 14px; }
  .managed-error { overflow-wrap: anywhere; }
  .setting-note { margin-bottom: 16px; }
  .field-error { margin: 0 0 16px; }
</style>
