<script lang="ts">
  import type { ManagedVersion, Project } from '../lib/types';
  import { canActivateManagedVersion } from '../lib/ui';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  let { project, versions, loading, error, actionError, busy, preview, onclose, onrefresh, onopen, onactivate }: {
    project: Project | null; versions: ManagedVersion[]; loading: boolean; error: string;
    actionError: string; busy: boolean; preview: boolean; onclose: () => void; onrefresh: () => void;
    onopen: (downloadId: string) => void; onactivate: (downloadId: string) => Promise<boolean>;
  } = $props();
</script>

<Modal title="已准备版本" description={project?.name ?? '软件订阅已不存在'} {onclose}>
  <div class="modal-form">
    {#if preview}<p class="field-hint">示例版本，预览中不能切换。</p>{/if}
    {#if actionError}<p class="field-error" role="alert">{actionError}</p>{/if}
    {#if !project}<p>软件已取消订阅，重新订阅后可继续管理已有版本。</p>
    {:else if project.installedSource !== 'managed'}<p>此软件由外部安装，请修改本机版本记录。</p>
    {:else if loading}<p role="status">正在读取版本…</p>
    {:else if error}<p class="field-error" role="alert">{error}</p>
    {:else}<div class="managed-list">
      {#each versions as version (version.downloadId)}
        <article class="managed-version"><div class="managed-heading"><strong>{version.version}</strong>{#if version.active}<span class="status-badge success">当前版本</span>{:else}<button class="button secondary small" disabled={busy || preview || !canActivateManagedVersion(project, version, true)} onclick={() => void onactivate(version.downloadId)}>设为当前版本</button>{/if}</div>
          <details><summary>程序目录</summary><p>{version.path}</p><button class="text-button" disabled={busy || preview} onclick={() => onopen(version.downloadId)}><Icon name="folder" size={14} />打开目录</button></details>
        </article>
      {:else}<p>还没有已准备的版本。</p>{/each}
    </div>{/if}
    <div class="modal-footer"><button class="button secondary" disabled={busy || loading || !project || project.installedSource !== 'managed'} onclick={onrefresh}>刷新</button><button class="button primary" disabled={busy} onclick={onclose}>完成</button></div>
  </div>
</Modal>
<style>
  .managed-list { display: flex; flex-direction: column; }
  .managed-version { padding: 14px 0; border-bottom: 1px solid var(--border); }
  .managed-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  details { margin-top: 10px; color: var(--muted); font-size: 12px; }
  summary { cursor: pointer; }
  details p { overflow-wrap: anywhere; margin: 8px 0; user-select: text; }
</style>
