<script lang="ts">
  import type { BackupRecord, Project, ProjectChanges } from '../lib/types';
  import BackupPanel from './BackupPanel.svelte';
  import Modal from './Modal.svelte';
  let { project, revision, busy, preview, error, onclose, onupdate, onrestore, ondelete }: {
    project: Project; revision: number; busy: boolean; preview: boolean; error: string;
    onclose: () => void; onupdate: (id: string, changes: ProjectChanges) => Promise<void>;
    onrestore: (backup: BackupRecord) => void; ondelete: (backup: BackupRecord) => void;
  } = $props();
</script>

<Modal title="管理备份" description={project.name} {onclose}>
  <div class="modal-form">
    <label class="backup-toggle"><strong>更新前自动备份</strong><input type="checkbox" role="switch" aria-label="更新前自动备份" checked={project.backupEnabled ?? false} disabled={busy || preview || project.managementMode === 'files'} onchange={(event) => void onupdate(project.id, { backupEnabled: event.currentTarget.checked })} /></label>
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
    {#key `${project.id}:${revision}`}<BackupPanel projectId={project.id} {busy} {preview} {onrestore} {ondelete} />{/key}
    <div class="modal-footer"><button class="button secondary" disabled={busy} onclick={onclose}>关闭</button></div>
  </div>
</Modal>

<style>
  .modal-form > label.backup-toggle { display: flex; align-items: center; justify-content: space-between; gap: 20px; }
  .backup-toggle input { appearance: none; position: relative; width: 34px; height: 19px; margin: 0; border: none; border-radius: 12px; background: var(--border-strong); flex: none; cursor: pointer; }
  .backup-toggle input::before { content: ''; position: absolute; left: 2px; top: 2px; width: 15px; height: 15px; border-radius: 50%; background: var(--surface); transition: transform .14s; }
  .backup-toggle input:checked { background: var(--accent); }
  .backup-toggle input:checked::before { transform: translateX(15px); background: var(--accent-ink); }
  .backup-toggle input:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; }
  .backup-toggle input:disabled { opacity: .5; cursor: default; }
</style>
