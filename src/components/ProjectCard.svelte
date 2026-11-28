<script lang="ts">
  import type { Project, RepositoryNote, LibraryLayout } from '../lib/types';
  import { projectInitials, isFilesProject, projectPresentation } from '../lib/ui';

  let { project, note = null, focused = false, busy = false, onselect, onOpen, onFiles, oncontext, layout = 'three' }: {
    project: Project;
    note?: RepositoryNote | null;
    focused?: boolean;
    busy?: boolean;
    layout?: LibraryLayout;
    onselect: (project: Project) => void;
    onOpen?: (project: Project) => void;
    onFiles?: (project: Project) => void;
    oncontext: (project: Project, anchor: HTMLElement, x?: number, y?: number) => void;
  } = $props();

  const presentation = $derived(projectPresentation(project, note));
  const avatar = $derived.by(() => {
    if (!project.avatarUrl) return null;
    try { const url = new URL(project.avatarUrl); return url.protocol === 'https:' ? url.href : null; } catch { return null; }
  });
  let failedAvatar = $state<string | null>(null);

  function open() {
    if (!busy) (isFilesProject(project) ? onFiles ?? onselect : onOpen ?? onselect)(project);
  }

  function keys(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      onselect(project);
    }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault();
      oncontext(project, event.currentTarget as HTMLElement);
    }
  }
</script>

<article
  class="project-card"
  class:focused
  class:layout-two={layout === 'two'}
  class:layout-three={layout === 'three'}
  class:layout-list={layout === 'list'}
  aria-label={presentation.label}
>
  <button
    class="card-identity"
    aria-label={presentation.label}
    aria-keyshortcuts="Enter"
    onkeydown={keys}
    ondblclick={open}
    oncontextmenu={(event) => {
      event.preventDefault();
      oncontext(project, event.currentTarget, event.clientX, event.clientY);
    }}
  >
    <span class="card-avatar" aria-hidden="true">
      {#if avatar && failedAvatar !== avatar}
        <img src={avatar} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => (failedAvatar = avatar)} />
      {:else}
        {projectInitials(project)}
      {/if}
    </span>
    <div class="card-content">
      {#if !project.owner && !project.repo}<span class="local-badge">{project.repositoryMatch === 'pending' ? '识别中' : '本地'}</span>{/if}
      {#if presentation.title}
        <strong title={presentation.title}>{presentation.title}</strong>
      {/if}
      {#if presentation.annotated && presentation.description}
        <small title={presentation.description}>{presentation.description}</small>
      {/if}
    </div>
  </button>
  <button
    class="card-more"
    aria-label={`${presentation.label} 的更多操作`}
    aria-haspopup="menu"
    onclick={(event) => oncontext(project, event.currentTarget)}
    title={`${presentation.label} 的更多操作`}
  >
    <span aria-hidden="true">···</span>
  </button>
</article>

<style>
  /* Soft-UI card: raised, same colour as the page; hover lifts, press sinks 2px. */
  .project-card {
    min-width: 0;
    position: relative;
    border: 0;
    border-radius: var(--r-card, 24px);
    background: var(--neu-bg, var(--surface));
    box-shadow: var(--raise);
    transition: transform 220ms var(--spring), box-shadow 180ms ease;
  }
  .local-badge { position: absolute; top: 8px; left: 12px; font-size: 11px; padding: 2px 7px; border-radius: 9px; color: var(--text); background: var(--accent-soft); }
  .project-card:hover { transform: translateY(-2px); box-shadow: var(--raise-hover); }
  .project-card:active { transform: translateY(2px); box-shadow: var(--inset); transition-duration: 120ms; }
  .project-card.focused,
  .project-card:focus-within { box-shadow: var(--inset); transform: translateY(2px); }
  .project-card.focused::after { content: ""; position: absolute; top: 16px; left: 16px; width: 8px; height: 8px; border-radius: 50%; background: var(--cobalt); box-shadow: var(--cobalt-glow); pointer-events: none; }
  .card-identity {
    width: 100%;
    min-height: 184px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 24px 16px;
    border-radius: var(--r-card, 24px);
    outline-offset: 2px;
    user-select: none;
    text-align: center;
  }
  .card-avatar {
    width: 68px;
    height: 68px;
    display: grid;
    place-items: center;
    border-radius: 18px;
    background: var(--neu-bg);
    color: var(--subtle);
    font-size: 24px;
    font-weight: 650;
    overflow: hidden;
    box-shadow: var(--raise-sm);
    flex-shrink: 0;
  }
  .card-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .card-content {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    align-items: center;
    width: 100%;
    gap: 4px;
    padding: 0 8px; /* 增加内边距避免边缘溢出 */
  }
  strong {
    max-width: 100%;
    min-height: 22px;
    color: var(--heading);
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    line-height: 1.4;
  }
  small {
    max-width: 100%;
    font-size: 11px; /* 进一步减小字号，确保卡片不被撑破 */
    color: var(--muted);
    white-space: normal; /* 允许换行以防一行过长撑破 */
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2; /* 最多显示两行 */
    line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.3;
  }
  .card-more {
    position: absolute;
    right: 10px;
    top: 10px;
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--muted);
    font-size: 20px;
    line-height: 1;
    opacity: 0;
    transition: opacity 0.15s ease, background 0.15s ease, color 0.15s ease;
    z-index: 10; /* 确保不被遮挡 */
  }
  .project-card:hover .card-more,
  .project-card:focus-within .card-more {
    opacity: 1;
  }
  .card-more:hover {
    background: var(--accent-soft);
    color: var(--accent);
  }
  @media (hover: none) {
    .card-more {
      opacity: 1;
    }
  }
  @media (max-width: 1100px) {
    .card-identity {
      min-height: 160px;
      padding: 20px 12px;
      gap: 12px;
    }
    .card-avatar {
      width: 56px;
      height: 56px;
      border-radius: 14px;
      font-size: 20px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .project-card {
      transition: none;
      transform: none !important;
    }
  }

  /* Layout 差异化微调 */
  .layout-two .card-identity {
    min-height: 196px;
    padding: 28px 24px;
  }
  .layout-two .card-avatar {
    width: 76px;
    height: 76px;
    border-radius: 18px;
    font-size: 26px;
  }
  .layout-two strong {
    font-size: 15px;
  }

  /* Layout List 模式 */
  .layout-list {
    display: flex;
    flex-direction: row;
    align-items: center;
    padding: 0;
    border-radius: 18px;
  }
  .layout-list:hover {
    transform: translateX(2px);
  }
  .layout-list.focused::after {
    top: 50%;
    left: auto;
    right: 54px;
    transform: translateY(-50%);
  }
  .layout-list .card-identity {
    flex-direction: row;
    min-height: 56px;
    padding: 10px 56px 10px 14px;
    border-radius: 18px;
    justify-content: flex-start;
    gap: 14px;
    text-align: left;
  }
  .layout-list .card-avatar {
    width: 36px;
    height: 36px;
    border-radius: 12px;
    font-size: 14px;
  }
  .layout-list .card-content {
    align-items: flex-start;
    gap: 2px;
  }
  .layout-list strong {
    font-size: 13.5px;
    min-height: auto;
  }
  .layout-list small {
    font-size: 11.5px;
  }
  .layout-list .card-more {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    opacity: 0.6;
    flex-shrink: 0;
  }
  .layout-list:hover .card-more {
    opacity: 1;
  }
</style>
