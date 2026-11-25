<script lang="ts">
  import type { Project } from '../lib/types';
  import { formatBytes, formatDate, isFilesProject, projectInitials, releaseSummary, selectedDownloadAsset } from '../lib/ui';
  import Icon from './Icon.svelte';
  let { projects, readTags, selected, busy, ontoggle, onselect, ondownload }: {
    readTags?: Record<string, string>;
    projects: Project[]; selected: Set<string>; busy: boolean; ontoggle: (id: string) => void;
    onselect: (project: Project) => void; ondownload: (project: Project) => void;
  } = $props();
</script>

<div class="updates-queue" aria-label="待更新队列">
  {#each projects as project (project.id)}
    {@const files = isFilesProject(project)}
    {@const asset = selectedDownloadAsset(project)}
    <article class="update-item" class:read={!!project.release && readTags?.[project.id] === project.release.tag} class:selected={selected.has(project.id)}>
      <input type="checkbox" checked={selected.has(project.id)} aria-label={`选择更新 ${project.name}`} onchange={() => ontoggle(project.id)} />
      <span class="update-avatar">{projectInitials(project)}</span>
      <div class="update-content">
        <div class="update-heading"><button class="plain-link" onclick={() => onselect(project)}>{project.name}</button><span class="update-kind">{files ? '文件' : '软件'}</span></div>
        <p class="update-source">{project.owner}/{project.repo}</p>
        <div class="version-transition" aria-label={`${project.name} ${files ? '新发行' : '版本变化'}`}>
          {#if !files}<span>{project.installedVersion ?? '本机版本未记录'}</span><Icon name="arrowRight" size={15} />{:else}<span>最新发行</span>{/if}<strong>{project.release?.tag ?? '未知'}</strong>
          {#if project.release?.prerelease}<span class="preview-label">预览版</span>{/if}
        </div>
        <p class="update-file">{asset ? `${asset.name} · ${formatBytes(asset.size)}` : '未选发行文件'}{#if project.release}<span> · {formatDate(project.release.publishedAt)}</span>{/if}</p>
        {#if releaseSummary(project.release?.body ?? '')}<p class="update-notes">{releaseSummary(project.release?.body ?? '')}</p>{/if}
        <div class="update-actions"><button class="button secondary small" onclick={() => onselect(project)}><Icon name="notes" size={14} />查看发行说明</button><button class="button primary small" disabled={busy || !asset} onclick={() => ondownload(project)}><Icon name="download" size={14} />{files ? '下载文件' : '下载更新'}</button></div>
      </div>
    </article>
  {/each}
</div>

<style>
  .update-item { display:flex; align-items:flex-start; gap:14px; padding:22px; border-top:1px solid var(--border); }
  .update-item.selected { background:var(--surface-soft); }
  .update-item > input { margin-top:11px; flex-shrink:0; accent-color:var(--accent); }
  .update-avatar { width:44px; height:44px; flex:0 0 44px; display:grid; place-items:center; border:1px solid var(--border); border-radius:10px; color:var(--subtle); background:var(--surface-soft); font-weight:650; }
  .update-content { min-width:0; flex:1; }
  .update-heading { display:flex; gap:9px; align-items:center; flex-wrap:wrap; }
  .update-heading .plain-link { font-size:17px; font-weight:650; color:var(--heading); overflow-wrap:anywhere; text-align:left; }
  .update-heading .plain-link:hover { color:var(--accent); }
  .update-kind, .update-source { color:var(--muted); font-size:12px; }
  .update-source { margin-top:4px; overflow-wrap:anywhere; }
  .version-transition { display:flex; flex-wrap:wrap; align-items:center; gap:10px; margin-top:14px; font-size:14px; font-variant-numeric:tabular-nums; overflow-wrap:anywhere; }
  .version-transition > span { color:var(--subtle); }
  .version-transition strong { color:var(--accent); font-size:16px; }
  .read .version-transition strong { color:var(--subtle); }
  .version-transition .preview-label { color:var(--warning); font-size:11px; }
  .update-file { margin-top:7px; font-size:12px; color:var(--muted); line-height:1.6; overflow-wrap:anywhere; }
  .update-notes { margin-top:13px; max-width:75ch; line-height:1.65; color:var(--subtle); font-size:13px; overflow-wrap:anywhere; }
  .update-actions { display:flex; flex-wrap:wrap; gap:9px; margin-top:15px; }
  @media (max-width:1100px) { .update-item { padding:18px; gap:10px; } .update-avatar { display:none; } }
</style>
