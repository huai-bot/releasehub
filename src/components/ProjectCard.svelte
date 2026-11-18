<script lang="ts">
  import type { Project } from '../lib/types';
  import { formatBytes, formatDate, getProjectStatus, hasProjectUpdate, isFilesProject, projectInitials, selectedDownloadAsset } from '../lib/ui';
  import Icon from './Icon.svelte';
  let { project, selected = false, focused = false, busy = false, ontoggle, onselect, ondownload, oncheck, onOpen, onFiles, onBackup }: {
    project: Project; selected?: boolean; focused?: boolean; busy?: boolean;
    ontoggle: (id: string) => void; onselect: (project: Project) => void;
    ondownload: (project: Project) => void; oncheck: (ids: string[]) => void;
    onOpen?: (project: Project) => void; onFiles?: (project: Project) => void; onBackup?: (project: Project) => void;
  } = $props();
  const files = $derived(isFilesProject(project));
  const status = $derived(getProjectStatus(project));
  const asset = $derived(selectedDownloadAsset(project));
  const avatar = $derived.by(() => {
    try { const url = new URL(project.avatarUrl ?? ''); return url.protocol === 'https:' ? url.href : null; } catch { return null; }
  });
  let failedAvatar = $state<string | null>(null);
</script>

<article class="project-card" class:checked={selected} class:focused aria-label={project.name}>
  <div class="card-top">
    <button class="card-identity" onclick={() => onselect(project)} aria-label={`查看 ${project.name} 详情`}>
      <span class="card-avatar">{#if avatar && failedAvatar !== avatar}<img src={avatar} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => failedAvatar = avatar} />{:else}{projectInitials(project)}{/if}</span>
      <span class="identity-copy"><strong>{project.name}</strong><span>{project.owner}/{project.repo}</span></span>
    </button>
    <input type="checkbox" aria-label={`选择 ${project.name}`} checked={selected} onchange={() => ontoggle(project.id)} />
  </div>
  <div class="card-labels"><span class="resource-kind"><Icon name={files ? 'notes' : 'monitor'} size={13} />{files ? '文件订阅' : '软件'}</span><span class={`status-badge ${status.tone}`}>{status.label}</span>{#if project.channel === 'preview'}<span class="channel-label">含预览版</span>{/if}</div>
  <p class="card-description">{project.description || '作者暂未填写项目说明。'}</p>
  <dl class="card-versions">
    {#if !files}<div><dt>本机版本</dt><dd>{project.installedVersion ?? '未记录'}</dd></div>{/if}
    <div><dt>最新发行</dt><dd class:update={hasProjectUpdate(project)}>{project.release?.tag ?? '待检查'}</dd></div>
  </dl>
  <div class="card-file"><Icon name={files ? 'notes' : 'package'} size={14} /><span title={asset?.name}>{asset ? `${asset.name} · ${formatBytes(asset.size)}` : files ? '查看详情，选择要保存的附件' : project.release ? '查看详情，选择适用的安装包' : '检查更新后查看发行附件'}</span></div>
  {#if project.error}<p class="card-error"><Icon name="alert" size={14} />{project.error}</p>{/if}
  <div class="card-footer"><span>{project.checkedAt ? `${formatDate(project.checkedAt)} 检查` : '尚未检查'}</span><div class="card-actions">
    <button class="button secondary small" onclick={() => onselect(project)}>查看详情</button>
    {#if files}<button class="button secondary small" disabled={busy} onclick={() => (onFiles ?? onselect)(project)}><Icon name="folder" size={14} />查看文件</button>
    {:else}<button class="button secondary small" disabled={busy || !onOpen} onclick={() => onOpen?.(project)}><Icon name="external" size={14} />打开软件</button>{/if}
    {#if !files && onBackup}<button class="button secondary small" disabled={busy} onclick={() => onBackup?.(project)}><Icon name="drive" size={14} />管理备份</button>{/if}
    {#if asset && hasProjectUpdate(project)}<button class="button primary small" disabled={busy} onclick={() => ondownload(project)}><Icon name="download" size={14} />{files ? '下载文件' : '下载更新'}</button>
    {:else if project.status === 'notChecked' || project.status === 'error'}<button class="icon-button" aria-label={`检查 ${project.name} 更新`} disabled={busy} onclick={() => oncheck([project.id])}><Icon name="refresh" size={15} /></button>{/if}
  </div></div>
</article>

<style>
  .project-card { min-width:0; display:flex; flex-direction:column; padding:20px; border:1px solid var(--border); border-radius:14px; background:var(--surface); }
  .project-card.checked { border-color:var(--accent); }
  .project-card.focused { background:var(--surface-soft); }
  .card-top { display:flex; align-items:flex-start; gap:12px; }
  .card-top input { margin-top:5px; flex-shrink:0; accent-color:var(--accent); }
  .card-identity { min-width:0; flex:1; display:flex; align-items:center; gap:13px; text-align:left; border:0; background:none; color:inherit; padding:0; border-radius:6px; }
  .card-identity:hover strong { color:var(--accent); }
  .card-avatar { width:52px; height:52px; flex:0 0 52px; display:grid; place-items:center; border-radius:12px; border:1px solid var(--border); background:var(--surface-soft); color:var(--subtle); font-weight:700; font-size:18px; overflow:hidden; }
  .card-avatar img { width:100%; height:100%; object-fit:cover; }
  .identity-copy { min-width:0; display:flex; flex-direction:column; gap:5px; }
  .identity-copy strong { font-size:18px; line-height:1.25; font-weight:650; overflow-wrap:anywhere; color:var(--heading); }
  .identity-copy > span { color:var(--muted); font-size:12px; overflow-wrap:anywhere; }
  .card-labels { display:flex; align-items:center; flex-wrap:wrap; gap:8px; margin-top:16px; }
  .resource-kind { display:flex; align-items:center; gap:5px; color:var(--muted); font-size:12px; }
  .channel-label { color:var(--muted); font-size:11px; }
  .card-description { margin:13px 0 16px; color:var(--subtle); font-size:13px; line-height:1.65; min-height:43px; display:-webkit-box; line-clamp:2; -webkit-line-clamp:2; -webkit-box-orient:vertical; overflow:hidden; overflow-wrap:anywhere; }
  .card-versions { display:flex; flex-wrap:wrap; gap:20px; margin:0; }
  .card-versions div { min-width:0; flex:1; }
  dt { color:var(--muted); font-size:12px; margin-bottom:5px; }
  dd { margin:0; font-size:15px; font-weight:600; font-variant-numeric:tabular-nums; overflow-wrap:anywhere; }
  dd.update { color:var(--accent); }
  .card-file { display:flex; align-items:flex-start; gap:6px; margin-top:14px; color:var(--muted); font-size:12px; line-height:1.5; }
  .card-file span { min-width:0; overflow-wrap:anywhere; }
  .card-file :global(svg) { flex-shrink:0; margin-top:2px; }
  .card-error { display:flex; align-items:flex-start; gap:6px; margin-top:12px; color:var(--danger); font-size:12px; overflow-wrap:anywhere; }
  .card-footer { display:flex; flex-direction:column; gap:12px; margin-top:auto; padding-top:18px; }
  .card-footer > span { color:var(--muted); font-size:11px; }
  .card-actions { display:flex; gap:8px; flex-wrap:wrap; }
</style>
