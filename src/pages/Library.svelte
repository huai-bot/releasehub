<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { LibraryLayout, Project, RepositoryNote } from '../lib/types';
  import { canDownloadAsset, hasProjectUpdate, isFilesProject, matchesProject, projectPresentation } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import ProjectCard from '../components/ProjectCard.svelte';
  import UpdatesQueue from '../components/UpdatesQueue.svelte';

  let {
    projects,
    updatesOnly = false,
    active = true,
    query,
    selectedId,
    busy,
    layout = 'three',
    notes = [],
    onlayout,
    onnote,
    onadd,
    onselect,
    oncheck,
    ondownload,
    onbatchdownload,
    onOpen,
    onFiles,
    onBackup,
    onRepository,
  }: {
    projects: Project[];
    updatesOnly?: boolean;
    active?: boolean;
    query: string;
    selectedId: string | null;
    busy: boolean;
    layout?: LibraryLayout;
    notes?: RepositoryNote[];
    onlayout?: (layout: LibraryLayout) => void;
    onnote?: (project: Project) => void;
    onadd: () => void;
    onselect: (project: Project) => void;
    oncheck: (ids?: string[]) => void;
    ondownload: (project: Project) => void;
    onbatchdownload: (projects: Project[]) => void;
    onOpen?: (project: Project) => void;
    onFiles?: (project: Project) => void;
    onBackup?: (project: Project) => void;
    onRepository?: (project: Project) => void;
  } = $props();

  let resourceType = $state<'all' | 'software' | 'files'>(untrack(() => (updatesOnly ? 'all' : 'software')));
  let filter = $state('all');
  let selected = $state(new Set<string>());

  const notesMap = $derived(new Map(notes.map((n) => [`${n.repository}`.toLowerCase(), n])));
  function getNote(project: Project) {
    return notesMap.get(`${project.owner}/${project.repo}`.toLowerCase()) ?? null;
  }

  const base = $derived(projects.filter((project) => !updatesOnly || hasProjectUpdate(project)));
  const softwareCount = $derived(base.filter((project) => !isFilesProject(project)).length);
  const filesCount = $derived(base.filter(isFilesProject).length);
  const typed = $derived(
    base.filter((project) =>
      resourceType === 'all' ? true : isFilesProject(project) ? resourceType === 'files' : resourceType === 'software'
    )
  );
  const visible = $derived(
    typed.filter((project) => matchesProject(project, query, getNote(project)) && (filter === 'all' || project.channel === filter))
  );
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((project) => selected.has(project.id)));
  const chosen = $derived(projects.filter((project) => selected.has(project.id)));
  const downloadable = $derived(
    chosen.filter((project) => project.release?.assets.some((asset) => asset.id === project.selectedAssetId && canDownloadAsset(asset)))
  );

  let menu = $state<{ project: Project; x: number; y: number; anchor: HTMLElement } | null>(null);
  let menuElement = $state<HTMLDivElement>();

  const menuPresentation = $derived.by(() => {
    if (!menu) return null;
    return projectPresentation(menu.project, getNote(menu.project));
  });

  $effect(() => {
    const allowed = new Set(base.map((project) => project.id));
    if ([...selected].some((id) => !allowed.has(id))) {
      selected = new Set([...selected].filter((id) => allowed.has(id)));
    }
  });

  $effect(() => {
    if (menu && (!active || !visible.some((project) => project.id === menu?.project.id))) {
      menu = null;
    }
  });

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function toggleAll() {
    const next = new Set(selected);
    for (const project of visible) {
      if (allVisibleSelected) next.delete(project.id);
      else next.add(project.id);
    }
    selected = next;
  }

  async function openMenu(project: Project, anchor: HTMLElement, x?: number, y?: number) {
    const rect = anchor.getBoundingClientRect();
    const next = { project, anchor, x: x ?? rect.left, y: y ?? rect.bottom + 4 };
    menu = next;
    await tick();
    if (menu?.anchor !== anchor || !menuElement) return;
    const bounds = menuElement.getBoundingClientRect();
    menu = {
      ...next,
      x: Math.max(8, Math.min(next.x, window.innerWidth - bounds.width - 8)),
      y: Math.max(8, Math.min(next.y, window.innerHeight - bounds.height - 8)),
    };
    menuElement.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }

  function closeMenu(restore = false) {
    const anchor = menu?.anchor;
    menu = null;
    if (restore) anchor?.focus();
  }

  function perform(action: ((project: Project) => void) | undefined) {
    const project = menu?.project;
    closeMenu(true);
    if (project) action?.(project);
  }

  function menuKeys(event: KeyboardEvent) {
    if (!menu) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
      return;
    }
    if (event.key === 'Tab') {
      closeMenu(true);
      return;
    }
    const buttons = [...(menuElement?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next = index;
    if (event.key === 'ArrowDown') next = (index + 1) % buttons.length;
    else if (event.key === 'ArrowUp') next = (index - 1 + buttons.length) % buttons.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = buttons.length - 1;
    else return;
    event.preventDefault();
    buttons[next]?.focus();
  }
</script>

<svelte:window
  onpointerdown={(event) => {
    if (menu && !menuElement?.contains(event.target as Node)) closeMenu();
  }}
  onresize={() => closeMenu()}
  onblur={() => closeMenu()}
/>
<svelte:document
  onscrollcapture={(event) => {
    if (menu && !menuElement?.contains(event.target as Node)) closeMenu();
  }}
/>

<div class="page-heading">
  <h1>{updatesOnly ? '可更新' : '资源库'}<span class="heading-count">{base.length}</span></h1>
  {#if updatesOnly}
    <div class="heading-actions">
      {#if base.length}
        <button class="button secondary" disabled={busy} onclick={() => oncheck(base.map((project) => project.id))}>
          <Icon name="refresh" class={busy ? 'spinning' : ''} size={16} />重新检查
        </button>
      {/if}
      <button class="button primary" onclick={onadd}><Icon name="plus" size={17} />添加资源</button>
    </div>
  {/if}
</div>

<section class:surface={updatesOnly} class:library-gallery={!updatesOnly} class="library-surface">
  <div class="list-toolbar resource-toolbar">
    <div class="segmented-tabs" role="group" aria-label="资源类型筛选">
      {#if updatesOnly}
        <button class:active={resourceType === 'all'} aria-pressed={resourceType === 'all'} onclick={() => (resourceType = 'all')}>
          全部<span>{base.length}</span>
        </button>
      {/if}
      <button class:active={resourceType === 'software'} aria-pressed={resourceType === 'software'} onclick={() => (resourceType = 'software')}>
        软件<span>{softwareCount}</span>
      </button>
      <button class:active={resourceType === 'files'} aria-pressed={resourceType === 'files'} onclick={() => (resourceType = 'files')}>
        文件<span>{filesCount}</span>
      </button>
    </div>

    {#if updatesOnly}
      <div class="resource-filter-actions">
        <select aria-label="渠道筛选" bind:value={filter}>
          <option value="all">全部渠道</option>
          <option value="stable">正式版</option>
          <option value="preview">包含预览版</option>
        </select>
        <button
          class="icon-button"
          aria-label="检查列表中的资源"
          title="检查当前筛选结果"
          disabled={busy || !visible.length}
          onclick={() => oncheck(visible.map((project) => project.id))}
        >
          <Icon name="refresh" class={busy ? 'spinning' : ''} size={17} />
        </button>
      </div>
    {:else}
      <div class="gallery-toolbar-right">
        <div class="segmented-tabs layout-switcher" role="group" aria-label="布局切换">
          <button
            class:active={layout === 'two'}
            aria-pressed={layout === 'two'}
            disabled={busy}
            onclick={() => onlayout?.('two')}
            aria-label="双列"
          >
            双列
          </button>
          <button
            class:active={layout === 'three'}
            aria-pressed={layout === 'three'}
            disabled={busy}
            onclick={() => onlayout?.('three')}
            aria-label="三列"
          >
            三列
          </button>
          <button
            class:active={layout === 'list'}
            aria-pressed={layout === 'list'}
            disabled={busy}
            onclick={() => onlayout?.('list')}
            aria-label="列表"
          >
            列表
          </button>
        </div>
      </div>
    {/if}
  </div>

  {#if updatesOnly && selected.size}
    <div class="bulk-toolbar">
      <span>已选 <strong>{selected.size}</strong> 项</span>
      <button class="text-button" disabled={busy} onclick={() => oncheck([...selected])}>
        <Icon name="refresh" size={14} />检查更新
      </button>
      <button class="text-button" disabled={busy || !downloadable.length} onclick={() => onbatchdownload(downloadable)}>
        <Icon name="download" size={14} />下载所选
        {#if downloadable.length !== chosen.length}
          <span>（{downloadable.length} 个可用）</span>
        {/if}
      </button>
      <button class="text-button clear-selection" onclick={() => (selected = new Set())}>取消选择</button>
    </div>
  {/if}

  {#if visible.length}
    {#if updatesOnly}
      <div class="selection-heading">
        <label>
          <input
            type="checkbox"
            aria-label="选择当前显示的所有资源"
            checked={allVisibleSelected}
            indeterminate={!allVisibleSelected && visible.some((project) => selected.has(project.id))}
            onchange={toggleAll}
          />
          选择当前 {visible.length} 项
        </label>
      </div>
      <UpdatesQueue projects={visible} {selected} {busy} {onselect} ontoggle={toggle} {ondownload} />
    {:else}
      <div class="library-card-grid" class:layout-two={layout === 'two'} class:layout-three={layout === 'three'} class:layout-list={layout === 'list'}>
        {#each visible as project (project.id)}
          <ProjectCard
            {project}
            note={getNote(project)}
            {layout}
            focused={selectedId === project.id || menu?.project.id === project.id}
            {busy}
            {onselect}
            {onOpen}
            {onFiles}
            oncontext={openMenu}
          />
        {/each}
      </div>
    {/if}
  {:else}
    <div class="empty-state spacious">
      <span class="empty-icon"><Icon name={query ? 'search' : updatesOnly ? 'success' : 'library'} size={29} /></span>
      <h3>
        {query
          ? '没有匹配的资源'
          : updatesOnly
            ? '当前没有待更新的资源'
            : `还没有${resourceType === 'files' ? '文件订阅' : '软件'}`}
      </h3>
      {#if updatesOnly}
        <button class="button secondary small" disabled={busy} onclick={() => oncheck()}>
          <Icon name="refresh" size={15} />检查全部资源
        </button>
      {:else}
        <button class="button primary small" onclick={onadd}><Icon name="plus" size={15} />添加资源</button>
      {/if}
    </div>
  {/if}
</section>

{#if menu && menuPresentation}
  <div
    class="resource-menu"
    bind:this={menuElement}
    role="menu"
    tabindex="-1"
    aria-label={`${menuPresentation.label} 的操作`}
    style:left={`${menu.x}px`}
    style:top={`${menu.y}px`}
    onkeydown={menuKeys}
    oncontextmenu={(event) => event.preventDefault()}
  >
    <span class="menu-label" title={menuPresentation.label}>{menuPresentation.label}</span>
    <button
      role="menuitem"
      disabled={busy}
      onclick={() => perform(isFilesProject(menu!.project) ? onFiles ?? onselect : onOpen ?? onselect)}
    >
      <Icon name={isFilesProject(menu.project) ? 'folder' : 'play'} size={16} />
      {isFilesProject(menu.project) ? '查看文件' : '打开软件'}
    </button>
    <button role="menuitem" onclick={() => perform(onselect)}>
      <Icon name="info" size={16} />查看详情
    </button>
    {#if !isFilesProject(menu.project) && onBackup}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onBackup)}>
        <Icon name="drive" size={16} />管理备份
      </button>
    {/if}
    {#if onRepository}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onRepository)}>
        <Icon name="github" size={16} />查看 GitHub 仓库
      </button>
    {/if}
    {#if onnote}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onnote)}>
        <Icon name="edit" size={16} />编辑备注
      </button>
    {/if}
  </div>
{/if}

<style>
  .heading-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .selection-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 13px 20px;
    color: var(--muted);
    font-size: 12px;
    border-top: 1px solid var(--border);
  }
  .selection-heading label {
    display: flex;
    align-items: center;
    gap: 9px;
    cursor: pointer;
  }
  .library-gallery {
    background: transparent;
    border: 0;
    box-shadow: none;
  }
  .library-gallery .resource-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 18px;
    border-bottom: 1px solid var(--glass-border, var(--border));
    margin-bottom: 24px;
    gap: 12px;
    flex-wrap: wrap;
  }
  .gallery-toolbar-right {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: auto;
  }
  .layout-switcher {
    padding: 2px;
    background: var(--surface-soft);
    border-radius: 9px;
    border: 1px solid var(--border-soft);
  }
  .layout-switcher button {
    padding: 5px 12px;
    font-size: 12px;
    border-radius: 7px;
  }
  .library-card-grid {
    display: grid;
    gap: 20px;
  }
  .library-card-grid.layout-two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .library-card-grid.layout-three {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .library-card-grid.layout-list {
    grid-template-columns: 1fr;
    gap: 10px;
  }
  .resource-menu {
    position: fixed;
    z-index: 80;
    width: 224px;
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
    overflow: auto;
    padding: 8px;
    border-radius: 14px;
    border: 1px solid var(--glass-border, var(--border));
    background: var(--glass-panel, var(--surface));
    backdrop-filter: var(--glass-filter, none);
    box-shadow: 0 16px 48px rgba(14, 32, 57, 0.2);
  }
  .menu-label {
    display: block;
    padding: 7px 10px 10px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-bottom: 1px solid var(--border-soft);
    margin-bottom: 4px;
  }
  .resource-menu button {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border-radius: 8px;
    text-align: left;
    font-size: 13px;
    color: var(--text);
  }
  .resource-menu button:hover:not(:disabled),
  .resource-menu button:focus-visible {
    background: var(--accent-soft);
    color: var(--accent);
    outline-offset: -2px;
  }
  .resource-menu button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .menu-divider {
    height: 1px;
    background: var(--border-soft);
    margin: 5px 4px;
  }
  @media (max-width: 1100px) {
    .library-card-grid {
      gap: 14px;
    }
    .gallery-toolbar-right {
      width: 100%;
      justify-content: flex-end;
      margin-left: 0;
    }
  }
  @media (max-width: 768px) {
    .library-card-grid.layout-three {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .resource-toolbar {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
