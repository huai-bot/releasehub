<script lang="ts">
  import type { AssetPreference, Channel, ManagementMode, Project, ProjectChanges } from '../lib/types';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  let { project, busy, error, onclose, onupdate, onmark, onexecutable, onversions, ondiagnostic, onremove }: {
    project: Project; busy: boolean; error: string; onclose: () => void;
    onupdate: (id: string, changes: ProjectChanges) => Promise<void>; onmark: (project: Project) => void;
    onexecutable: (project: Project) => void; onversions: (projectId: string) => void;
    ondiagnostic: (projectId: string) => void; onremove: (project: Project) => void;
  } = $props();
  const files = $derived(project.managementMode === 'files');
</script>

<Modal title="项目设置" description={project.name} {onclose}>
  <div class="modal-form">
    {#if !files}<button class="button secondary version-shortcut" disabled={busy} onclick={() => onmark(project)}><Icon name="edit" size={15} />修改本机版本 <span>{project.installedVersion ?? '未记录'}</span></button>{/if}
    <label>管理方式<select value={project.managementMode ?? 'software'} disabled={busy} onchange={(event) => void onupdate(project.id, { managementMode: event.currentTarget.value as ManagementMode })}><option value="software">软件</option><option value="files">文件</option></select></label>
    <label>订阅渠道<select value={project.channel} disabled={busy} onchange={(event) => void onupdate(project.id, { channel: event.currentTarget.value as Channel })}><option value="stable">正式版</option><option value="preview">包含预览版</option></select></label>
    {#if !files}<label>安装包偏好<select value={project.preference} disabled={busy} onchange={(event) => void onupdate(project.id, { preference: event.currentTarget.value as AssetPreference })}><option value="auto">自动推荐</option><option value="portable">优先免安装包</option><option value="installer">优先安装程序</option></select></label>{/if}
    <details class="advanced-settings"><summary>更多设置</summary><div class="modal-form">
      <form class="pattern-setting" onsubmit={(event) => { event.preventDefault(); const value = String(new FormData(event.currentTarget).get('pattern') ?? '').trim(); void onupdate(project.id, { assetPattern: value || null }); }}><label>文件名筛选<input name="pattern" value={project.assetPattern ?? ''} maxlength="200" disabled={busy} placeholder={files ? '例如 .pdf' : '例如 win-x64'} /></label><div class="form-inline-end"><button class="button secondary small" disabled={busy}>保存筛选</button></div></form>
      {#if !files}<button class="text-button" disabled={busy} onclick={() => onexecutable(project)}>更换启动程序</button>{/if}
      {#if !files && project.installedSource === 'managed'}<button class="text-button" disabled={busy} onclick={() => onversions(project.id)}>管理已准备版本</button>{/if}
      <button class="text-button" disabled={busy} onclick={() => ondiagnostic(project.id)}>复制资料给 AI</button>
      <button class="text-button danger-text" disabled={busy} onclick={() => onremove(project)}>取消订阅</button>
    </div></details>
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
    <div class="modal-footer"><button class="button primary" disabled={busy} onclick={onclose}>完成</button></div>
  </div>
</Modal>
<style>
  .version-shortcut { margin-bottom: 18px; }
  .advanced-settings > div { padding: 16px 0 0; display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }
  .pattern-setting { width: 100%; }
  .pattern-setting label { display: flex; flex-direction: column; gap: 7px; }
  .pattern-setting input { width: 100%; }
  summary { color: var(--muted); cursor: pointer; }
  .button span { margin-left: auto; color: var(--muted); font-weight: 400; }
</style>
