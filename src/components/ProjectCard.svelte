<script lang="ts">
  import type { Project } from '../lib/types';
  import { projectInitials, isFilesProject } from '../lib/ui';
  let { project, focused = false, busy = false, onselect, onOpen, onFiles, oncontext }: {
    project: Project; focused?: boolean; busy?: boolean;
    onselect: (project: Project) => void;
    onOpen?: (project: Project) => void; onFiles?: (project: Project) => void;
    oncontext: (project: Project, anchor: HTMLElement, x?: number, y?: number) => void;
  } = $props();
  const avatar = $derived.by(() => {
    try { const url = new URL(project.avatarUrl ?? ''); return url.protocol === 'https:' ? url.href : null; } catch { return null; }
  });
  let failedAvatar = $state<string | null>(null);
  function open() { if (!busy) (isFilesProject(project) ? onFiles ?? onselect : onOpen ?? onselect)(project); }
  function keys(event: KeyboardEvent) {
    if (event.key === 'Enter') { event.preventDefault(); onselect(project); }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); oncontext(project, event.currentTarget as HTMLElement); }
  }
</script>

<article class="project-card" class:focused aria-label={project.name}>
  <button class="card-identity" title={`${project.name} · 双击${isFilesProject(project) ? '查看文件' : '打开软件'}，右键查看更多操作`} aria-label={`${project.name}，双击${isFilesProject(project) ? '查看文件' : '打开软件'}，Enter 查看详情`} onkeydown={keys} ondblclick={open} oncontextmenu={(event) => { event.preventDefault(); oncontext(project, event.currentTarget, event.clientX, event.clientY); }}>
    <span class="card-avatar">{#if avatar && failedAvatar !== avatar}<img src={avatar} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => failedAvatar = avatar} />{:else}{projectInitials(project)}{/if}</span>
    <strong>{project.name}</strong>
  </button>
  <button class="card-more" aria-label={`${project.name} 的更多操作`} aria-haspopup="menu" onclick={(event) => oncontext(project, event.currentTarget)} title="更多操作"><span aria-hidden="true">···</span></button>
</article>

<style>
  .project-card { min-width:0; position:relative; border:1px solid var(--glass-border, var(--border)); border-radius:18px; background:var(--glass-card, var(--surface)); box-shadow:var(--glass-shadow); transition:background .15s, border-color .15s; }
  .project-card:hover { background:var(--glass-card-hover, var(--surface-soft)); }
  .project-card.focused, .project-card:focus-within { border-color:var(--accent); background:var(--glass-card-selected, var(--surface-soft)); }
  .card-identity { width:100%; min-height:184px; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:18px; padding:28px 14px; border-radius:17px; user-select:none; }
  .card-avatar { width:70px; height:70px; display:grid; place-items:center; border-radius:17px; background:var(--accent-soft); color:var(--accent); font-size:24px; font-weight:650; overflow:hidden; box-shadow:0 3px 12px #1422380a; }
  .card-avatar img { width:100%; height:100%; object-fit:cover; }
  strong { max-width:100%; min-height:22px; color:var(--heading); font-size:14px; font-weight:550; overflow:hidden; white-space:nowrap; text-overflow:ellipsis; }
  .card-more { position:absolute; right:8px; top:8px; width:32px; height:28px; border-radius:8px; color:var(--muted); font-size:22px; line-height:1; opacity:0; }
  .project-card:hover .card-more, .project-card:focus-within .card-more { opacity:1; }
  .card-more:hover { background:var(--accent-soft); color:var(--accent); }
  @media (hover:none) { .card-more { opacity:1; } }
  @media (max-width:1100px) { .card-identity { min-height:160px; padding:24px 10px; gap:14px; } .card-avatar { width:58px; height:58px; border-radius:14px; } }
  @media (prefers-reduced-motion:reduce) { .project-card { transition:none; } }
</style>
