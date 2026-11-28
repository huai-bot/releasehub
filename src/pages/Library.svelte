<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import LibrarySortMenu from '../components/LibrarySortMenu.svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { desktopAvailable } from '../lib/bridge';
  import { emptyIndex, filterRows, libraryRows, prepareRows, sortRows, type IndexReply, type LibraryIndex } from '../lib/library-index';
  import { moveLibraryOrder, reconcileLibraryOrder, type DropEdge } from '../lib/library-order';
  import type { LibraryLayout, LibrarySort, ManagementMode, Project, RepositoryNote } from '../lib/types';
  import { canDownloadAsset, hasProjectUpdate, hasUnreadUpdate, isFilesProject, matchesProject, projectPresentation } from '../lib/ui';
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
    layout = 'three', sort = 'added', onsort, softwareOrder = [], filesOrder = [], onreorder,
    onlocal, onpicklocal, allowDrop = true, onsettings,
    notes = [],
    readTags,
    onMarkRead,
    onToggleUpdates,
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
    sort?: LibrarySort; onsort?: (sort: LibrarySort) => void;
    softwareOrder?: readonly string[]; filesOrder?: readonly string[];
    onreorder?: (kind: ManagementMode, order: string[]) => Promise<boolean>;
    onpicklocal?: () => void; onlocal?: (paths: string[]) => void; allowDrop?: boolean;
    onsettings?: (project: Project) => void;
    notes?: RepositoryNote[];
    readTags?: Record<string, string>;
    onMarkRead?: (projects: Project[]) => void;
    onToggleUpdates?: (project: Project) => void;
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

  const base = $derived(updatesOnly ? projects.filter(hasProjectUpdate) : projects);
  const unread = $derived(updatesOnly ? base.filter(project => hasUnreadUpdate(project, readTags)) : []);
  const byId = $derived(new Map(projects.map(p => [p.id, p])));
  const rows = $derived(libraryRows(projects, notesMap));
  let worker: Worker | undefined, sentRows: typeof rows | undefined, generation = 0;
  let indexed = $state.raw<LibraryIndex>(emptyIndex);
  let indexedFor = $state.raw<{ rows: typeof rows; query: string; manual: boolean } | null>(null);
  let indexing = $state(false);
  $effect(() => {
    if (updatesOnly || !active) return;
    const data = rows, q = query, order = sort === 'manual' ? 'added' : sort, manual = sort === 'manual', request = ++generation;
    const publish = (result: LibraryIndex) => { indexed = result; indexedFor = { rows: data, query: q, manual }; indexing = false; };
    const fallback = () => publish(filterRows(sortRows(prepareRows(data), order), q, manual));
    if (data.length < 120 || typeof Worker === 'undefined') { sentRows = undefined; fallback(); return; }
    try {
      worker ??= new Worker(new URL('../lib/library-index.worker.ts', import.meta.url), { type: 'module' });
      worker.onmessage = ({ data }: MessageEvent<IndexReply>) => { if (data.generation === generation) publish(data.result); };
      worker.onerror = () => { worker?.terminate(); worker = undefined; sentRows = undefined; if (request === generation) fallback(); };
      indexing = true;
      worker.postMessage({ generation: request, ...(sentRows === data ? {} : { rows: data }), query: q, sort: order, includeUnfiltered: manual });
      sentRows = data;
    } catch { fallback(); }
  });
  onDestroy(() => { generation++; worker?.terminate(); cancelPointer(); });
  const filesCount = $derived(updatesOnly ? base.filter(isFilesProject).length : indexed.filesCount);
  const softwareCount = $derived(updatesOnly ? base.length - filesCount : indexed.softwareCount);
  const resourceKind = $derived(resourceType === 'files' ? 'files' : 'software');
  const fullOrder = $derived(sort === 'manual' && !updatesOnly
    ? reconcileLibraryOrder((indexed.unfiltered ?? indexed)[resourceKind], resourceKind === 'files' ? filesOrder : softwareOrder)
    : indexed[resourceKind]);
  const ids = $derived.by(() => {
    if (sort !== 'manual' || !indexed.unfiltered) return fullOrder;
    const matching = new Set(indexed[resourceKind]);
    return fullOrder.filter(id => matching.has(id));
  });
  const pageSize = 48;
  let pageNumber = $state(0);
  $effect(() => { resourceType; query; sort; pageNumber = 0; });
  const pageCount = $derived(Math.max(1, Math.ceil(ids.length / pageSize)));
  const currentPage = $derived(Math.min(pageNumber, pageCount - 1));
  const visible = $derived(updatesOnly
    ? base.filter(p => (resourceType === 'all' || (isFilesProject(p) ? 'files' : 'software') === resourceType) && (filter === 'all' || p.channel === filter) && matchesProject(p, query, getNote(p)))
    : ids.slice(currentPage * pageSize, (currentPage + 1) * pageSize).map(id => byId.get(id)).filter((p): p is Project => !!p));

  let surface: HTMLElement, dragOver = $state(false);
  const manual = $derived(!updatesOnly && sort === 'manual');
  const orderHintId = $props.id();
  let savingOrder = $state(false);
  let orderStatus = $state('');
  const canReorder = $derived(manual && active && !!onreorder && !busy && !savingOrder && allowDrop && !dragOver
    && indexedFor?.rows === rows && indexedFor.query === query && indexedFor.manual);
  type PageDirection = -1 | 0 | 1;
  type OrderTarget = { id: string; edge: DropEdge };
  type PointerPress = { id: string; label: string; pointerId: number; startX: number; startY: number; x: number; y: number };
  let press: PointerPress | null = null;
  let pointer = $state.raw<{ id: string; label: string; target: OrderTarget | null; page: PageDirection } | null>(null);
  let pageTimer: ReturnType<typeof setTimeout> | undefined;
  let scrollFrame: number | undefined;

  $effect(() => {
    // A gesture is tied to the displayed result set and current geometry, not to its page.
    ids; query; layout; canReorder;
    untrack(cancelPointer);
  });
  $effect(() => { query; resourceKind; manual; orderStatus = ''; });

  async function commitOrder(id: string, label: string, target: OrderTarget, restoreFocus: boolean) {
    if (!canReorder || !onreorder) return;
    const moved = moveLibraryOrder(fullOrder, ids, id, target.id, target.edge);
    if (!moved) return;
    const kind = resourceKind, atQuery = query, atLayout = layout;
    const destination = Math.floor(moved.index / pageSize);
    const previous = destination === currentPage && typeof Element.prototype.animate === 'function'
      && !window.matchMedia('(prefers-reduced-motion: reduce)').matches
      ? new Map([...surface.querySelectorAll<HTMLElement>('[data-order-id]')]
          .map(cell => [cell.dataset.orderId, { x: cell.offsetLeft, y: cell.offsetTop }]))
      : null;
    savingOrder = true;
    orderStatus = '';
    try {
      if (!await onreorder(kind, moved.order)) { orderStatus = '顺序未保存，原有顺序不变。'; return; }
      if (!active || !manual || query !== atQuery || resourceKind !== kind) return;
      const samePage = destination === currentPage;
      pageNumber = destination;
      orderStatus = `“${label}”已移至第 ${moved.index + 1} 项，共 ${ids.length} 项。`;
      await tick();
      const cells = [...(surface?.querySelectorAll<HTMLElement>('[data-order-id]') ?? [])];
      // Measure only a completed, same-page reorder, not search, paging or automatic sorts.
      if (previous && samePage && layout === atLayout) {
        const moves = cells.flatMap(cell => {
          const start = previous.get(cell.dataset.orderId);
          if (!start) return [];
          const x = start.x - cell.offsetLeft, y = start.y - cell.offsetTop;
          return x || y ? [{ cell, x, y }] : [];
        });
        for (const { cell, x, y } of moves) {
          cell.animate([{ transform: `translate(${x}px, ${y}px)` }, { transform: 'none' }], { duration: 220, easing: 'cubic-bezier(.3, 1.32, .6, 1)' });
        }
      }
      const cell = cells.find(item => item.dataset.orderId === id);
      if (restoreFocus) cell?.querySelector<HTMLButtonElement>('.order-handle')?.focus({ preventScroll: true });
      cell?.scrollIntoView?.({ block: 'nearest' });
    } catch {
      orderStatus = '顺序未保存，原有顺序不变。';
    } finally { savingOrder = false; }
  }

  function stepOrder(project: Project, label: string, offset: -1 | 1) {
    const index = ids.indexOf(project.id), target = ids[index + offset];
    if (index >= 0 && target) void commitOrder(project.id, label, { id: target, edge: offset < 0 ? 'before' : 'after' }, true);
  }

  function orderKeys(event: KeyboardEvent, project: Project, label: string) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const offset = ['ArrowUp', 'ArrowLeft'].includes(event.key) ? -1 : ['ArrowDown', 'ArrowRight'].includes(event.key) ? 1 : 0;
    if (!offset) return;
    event.preventDefault();
    const index = ids.indexOf(project.id);
    if (canReorder && index >= 0 && !ids[index + offset]) orderStatus = offset < 0 ? '已是第一项。' : '已是最后一项。';
    else stepOrder(project, label, offset);
  }

  function startPointer(event: PointerEvent, id: string, label: string) {
    if (!canReorder || press || event.button !== 0 || !event.isPrimary) return;
    const { pointerId, clientX: x, clientY: y } = event;
    press = { id, label, pointerId, startX: x, startY: y, x, y };
  }

  function cancelPointer() {
    const pointerId = press?.pointerId;
    press = null;
    pointer = null;
    clearTimeout(pageTimer);
    if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
    scrollFrame = undefined;
    if (pointerId !== undefined && surface?.hasPointerCapture?.(pointerId)) surface.releasePointerCapture(pointerId);
  }

  function updatePointerTarget(x: number, y: number) {
    if (!pointer) return;
    const hit = document.elementFromPoint(x, y);
    const zone = hit?.closest<HTMLElement>('[data-order-page]');
    const direction = zone && surface.contains(zone) ? Number(zone.dataset.orderPage) as PageDirection : 0;
    if (direction !== pointer.page) {
      clearTimeout(pageTimer);
      if (direction) pageTimer = setTimeout(() => {
        const next = currentPage + direction;
        if (!canReorder || pointer?.page !== direction || next < 0 || next >= pageCount) return;
        pageNumber = next;
        surface.closest('main')?.scrollTo?.({ top: 0 });
      }, 520);
    }
    const cell = !direction ? hit?.closest<HTMLElement>('[data-order-id]') : null;
    let target: OrderTarget | null = null;
    if (cell && surface.contains(cell) && cell.dataset.orderId && cell.dataset.orderId !== pointer.id) {
      const rect = cell.getBoundingClientRect();
      const before = layout === 'list' ? y < (rect.top + rect.bottom) / 2 : x < (rect.left + rect.right) / 2;
      target = { id: cell.dataset.orderId, edge: before ? 'before' : 'after' };
    }
    if (direction !== pointer.page || target?.id !== pointer.target?.id || target?.edge !== pointer.target?.edge) {
      pointer = { ...pointer, page: direction, target };
    }
  }

  function scrollWhileDragging() {
    if (!press || !pointer) return;
    const container = surface.closest('main');
    if (container && !pointer.page) {
      const rect = container.getBoundingClientRect();
      const { x, y } = press;
      if (x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom) {
        const edge = 52;
        const amount = y < rect.top + edge ? y - rect.top - edge : y > rect.bottom - edge ? y - rect.bottom + edge : 0;
        const before = container.scrollTop;
        if (amount) container.scrollTop += Math.sign(amount) * Math.ceil(Math.abs(amount) / 4);
        if (container.scrollTop !== before) updatePointerTarget(x, y);
      }
    }
    scrollFrame = requestAnimationFrame(scrollWhileDragging);
  }

  function movePointer(event: PointerEvent) {
    if (!press || event.pointerId !== press.pointerId) return;
    if (!canReorder) { cancelPointer(); return; }
    press.x = event.clientX; press.y = event.clientY;
    if (!pointer) {
      if (Math.hypot(press.x - press.startX, press.y - press.startY) < 6) return;
      // The rail and grid survive a page turn even though the original handle does not.
      surface.setPointerCapture?.(event.pointerId);
      pointer = { id: press.id, label: press.label, target: null, page: 0 };
      scrollFrame = requestAnimationFrame(scrollWhileDragging);
    }
    updatePointerTarget(press.x, press.y);
  }

  function finishPointer(event: PointerEvent) {
    if (!press || event.pointerId !== press.pointerId) return;
    if (pointer) updatePointerTarget(event.clientX, event.clientY);
    const { id, label } = press;
    const target = pointer?.target;
    cancelPointer();
    if (target) void commitOrder(id, label, target, false);
  }

  function inside(x: number, y: number) {
    if (!surface || !active || !allowDrop || busy || updatesOnly || resourceType !== 'software') return false;
    const r = surface.closest('main')?.getBoundingClientRect() ?? surface.getBoundingClientRect();
    const scale = window.devicePixelRatio || 1;
    return x / scale >= r.left && x / scale < r.right && y / scale >= r.top && y / scale < r.bottom;
  }
  onMount(() => {
    if (updatesOnly || !onlocal || !desktopAvailable()) return;
    let disposed = false, release: (() => void) | undefined;
    try {
      void getCurrentWebview().onDragDropEvent(({ payload }) => {
        if (disposed) return;
        if (payload.type === 'leave') { dragOver = false; return; }
        const over = inside(payload.position.x, payload.position.y);
        dragOver = over && payload.type !== 'drop';
        if (over && payload.type === 'drop') onlocal?.(payload.paths);
      }).then(stop => { if (disposed) stop(); else release = stop; }).catch(() => {});
    } catch { /* The file chooser remains available if native drop is unavailable. */ }
    return () => { disposed = true; release?.(); };
  });
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
    if (!updatesOnly || !selected.size) return;
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

  async function turnPage(next: number) {
    pageNumber = next; await tick();
    surface?.closest('main')?.scrollTo?.({ top: 0 });
    surface?.querySelector<HTMLButtonElement>('.card-identity')?.focus({ preventScroll: true });
  }
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

  async function openMenu(project: Project, anchor: HTMLElement, x?: number, y?: number, orderFirst = false) {
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
    const first = orderFirst && menuElement.querySelector<HTMLButtonElement>('button[data-order-step]:not(:disabled)');
    (first || menuElement.querySelector<HTMLButtonElement>('button:not(:disabled)'))?.focus();
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
  onpointermove={movePointer}
  onpointerup={finishPointer}
  onpointercancel={(event) => { if (event.pointerId === press?.pointerId) cancelPointer(); }}
  onkeydown={(event) => { if (event.key === 'Escape' && press) { event.preventDefault(); cancelPointer(); } }}
  onresize={() => { closeMenu(); cancelPointer(); }}
  onblur={() => { closeMenu(); cancelPointer(); }}
/>
<svelte:document
  onscrollcapture={(event) => {
    if (menu && !menuElement?.contains(event.target as Node)) closeMenu();
  }}
/>

<div class="page-heading">
  <h1>{updatesOnly ? '可更新' : '资源库'}{#if !updatesOnly || unread.length}<span class="heading-count">{updatesOnly ? unread.length : base.length}</span>{/if}</h1>
  {#if updatesOnly}
    <div class="heading-actions">
      {#if base.length}
        {#if onMarkRead}<button class="button secondary" disabled={busy || !unread.length} onclick={() => onMarkRead?.(unread)}>
          <Icon name="check" size={16} />全部已读
        </button>{/if}
        <button class="button secondary" disabled={busy} onclick={() => oncheck(base.map((project) => project.id))}>
          <Icon name="refresh" class={busy ? 'spinning' : ''} size={16} />重新检查
        </button>
      {/if}
      <button class="button primary" onclick={onadd}><Icon name="plus" size={17} />添加资源</button>
    </div>
  {:else if resourceType === 'software' && onpicklocal}
    <button class="button secondary small" disabled={busy} onclick={onpicklocal} title="也可直接把 .exe 文件拖到资源库"><Icon name="plus" size={15} />添加本地软件</button>
  {/if}
</div>

<section bind:this={surface} class:drop-ready={dragOver} class:surface={updatesOnly} class:list-surface={updatesOnly} class:library-gallery={!updatesOnly} class="library-surface">
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
        <LibrarySortMenu {sort} onsort={(value) => onsort?.(value)} />
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

  {#if dragOver}<div class="drop-prompt" role="status"><Icon name="import" size={19} />松开以添加本地软件</div>{/if}
  {#if manual}
    <p class="sr-only" role="status" aria-live="polite" aria-atomic="true">{orderStatus}</p>
    {#if visible.length}
      <div class="order-rail" class:dragging={!!pointer}>
        <span><Icon name="grip" size={18} /><span id={orderHintId} class:sr-only={!!pointer}>拖动手柄，或聚焦后按方向键移动。{#if pageCount > 1}拖到分页键上停留可翻页。{/if}</span>
          {#if pointer}<span class="order-drag-label">正在移动：{pointer.label}</span>{/if}
        </span>
        {#if pageCount > 1}
          <nav class="order-pages" aria-label="手动排序分页">
            <button type="button" class="button secondary small" data-order-page={currentPage > 0 ? -1 : undefined}
              class:page-target={pointer?.page === -1} disabled={currentPage === 0} onclick={() => turnPage(currentPage - 1)}>上一页</button>
            <span aria-live="polite">{currentPage + 1} / {pageCount}</span>
            <button type="button" class="button secondary small" data-order-page={currentPage + 1 < pageCount ? 1 : undefined}
              class:page-target={pointer?.page === 1} disabled={currentPage + 1 >= pageCount} onclick={() => turnPage(currentPage + 1)}>下一页</button>
          </nav>
        {/if}
      </div>
    {/if}
  {/if}
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
      <UpdatesQueue projects={visible} {readTags} {selected} {busy} {onselect} ontoggle={toggle} {ondownload} />
    {:else}
      <div class="library-card-grid" class:layout-two={layout === 'two'} class:layout-three={layout === 'three'} class:layout-list={layout === 'list'}>
        {#each visible as project (project.id)}
          <div class="library-card-cell" data-order-id={manual ? project.id : undefined}
            class:order-source={pointer?.id === project.id}
            class:order-before={pointer?.target?.id === project.id && pointer.target.edge === 'before'}
            class:order-after={pointer?.target?.id === project.id && pointer.target.edge === 'after'}>
          {#snippet grip(label: string)}
            <button type="button" class="order-handle" class:held={pointer?.id === project.id}
              aria-label={`移动 ${label}`} aria-describedby={orderHintId}
              aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight" aria-haspopup="menu"
              aria-disabled={!canReorder || ids.length < 2} title="拖动或点击移动资源"
              onpointerdown={(event) => startPointer(event, project.id, label)}
              onkeydown={(event) => orderKeys(event, project, label)}
              onclick={(event) => { if (canReorder && ids.length > 1) void openMenu(project, event.currentTarget, undefined, undefined, true); }}>
              <Icon name="grip" size={19} />
            </button>
          {/snippet}
            <ProjectCard
              {project}
              note={getNote(project)}
              {layout}
              focused={selectedId === project.id || menu?.project.id === project.id}
              {busy}
              {onselect}
              {onOpen}
              {onFiles}
              orderHandle={manual ? grip : undefined}
              oncontext={openMenu}
            />
          </div>
        {/each}
      </div>
      {#if pageCount > 1 && !manual}<nav class="library-pages" aria-label="资源分页">
        <span>第 {currentPage + 1} / {pageCount} 页 · 共 {ids.length} 项</span>
        <button class="button secondary small" disabled={currentPage === 0} onclick={() => turnPage(currentPage - 1)}>上一页</button>
        <button class="button secondary small" disabled={currentPage + 1 >= pageCount} onclick={() => turnPage(currentPage + 1)}>下一页</button>
      </nav>{/if}
    {/if}
  {:else if indexing && !updatesOnly}<p class="index-hint" role="status">正在整理资源…</p>
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
      {#if !updatesOnly && resourceType === 'software' && !query}<p>把 .exe 拖到这里，或点“添加本地软件”。</p>{/if}
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
    {#if manual}
      {@const index = ids.indexOf(menu.project.id)}
      <button type="button" role="menuitem" data-order-step disabled={!canReorder || index <= 0}
        onclick={() => { if (menu && menuPresentation) stepOrder(menu.project, menuPresentation.label, -1); closeMenu(true); }}>
        <Icon name="arrowUp" size={16} />上移一位
      </button>
      <button type="button" role="menuitem" data-order-step disabled={!canReorder || index < 0 || index + 1 >= ids.length}
        onclick={() => { if (menu && menuPresentation) stepOrder(menu.project, menuPresentation.label, 1); closeMenu(true); }}>
        <Icon name="arrowDown" size={16} />下移一位
      </button>
      <div class="menu-divider"></div>
    {/if}
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
    {#if onsettings && !isFilesProject(menu.project)}<button role="menuitem" disabled={busy} onclick={() => perform(onsettings)}><Icon name="settings" size={16} />设置 GitHub 仓库</button>{/if}
    {#if menu.project.repo && onToggleUpdates}<button role="menuitem" disabled={busy} onclick={() => perform(onToggleUpdates)}>
      <Icon name={menu.project.updatesMuted ? 'eye' : 'eyeOff'} size={16} />{menu.project.updatesMuted ? '恢复更新' : '屏蔽更新'}
    </button>{/if}
    {#if !isFilesProject(menu.project) && onBackup}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onBackup)}>
        <Icon name="drive" size={16} />管理备份
      </button>
    {/if}
    {#if menu.project.repo && onRepository}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onRepository)}>
        <Icon name="github" size={16} />查看 GitHub 仓库
      </button>
    {/if}
    {#if menu.project.repo && onnote}
      <div class="menu-divider"></div>
      <button role="menuitem" disabled={busy} onclick={() => perform(onnote)}>
        <Icon name="edit" size={16} />编辑备注
      </button>
    {/if}
  </div>
{/if}

<style>
  .drop-ready { outline: 2px dashed var(--accent); outline-offset: 8px; }
  .drop-prompt { display: flex; align-items: center; gap: 8px; background: var(--accent-soft); padding: 12px; border-radius: 12px; margin: 0 0 16px; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .library-pages, .order-pages { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; color: var(--muted); font-size: 12px; }
  .library-pages { justify-content: flex-end; padding-top: 18px; }
  .order-rail {
    position: sticky; top: 0; z-index: 20;
    display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 16px;
    margin: -8px 0 18px; padding: 9px 12px;
    border-radius: var(--r-key, 15px);
    background: var(--neu-bg, var(--surface)); box-shadow: var(--inset-sm);
    color: var(--muted); font-size: 12px; line-height: 1.5;
  }
  .order-rail > span { display: flex; align-items: center; gap: 8px; flex: 1 1 230px; min-width: 0; }
  .order-rail > span :global(svg) { flex: none; }
  .order-rail.dragging { color: var(--text); }
  .order-drag-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-weight: 600; }
  .order-pages { flex: none; margin-inline-start: auto; }
  .order-pages > span { min-width: 48px; text-align: center; font-variant-numeric: tabular-nums; }
  .order-pages .page-target { outline: 2px solid var(--focus, var(--accent)); outline-offset: 2px; }
  .library-card-cell { min-width: 0; position: relative; }
  .library-card-cell[data-order-id] { scroll-margin-top: 76px; }
  .order-handle {
    position: absolute; z-index: 2; inset-block-start: 9px; inset-inline-start: 9px;
    display: grid; place-items: center; width: 34px; height: 34px; padding: 0;
    border-radius: 50%; color: var(--subtle); background: var(--neu-bg, var(--surface));
    box-shadow: var(--raise-sm); cursor: grab; user-select: none; touch-action: none;
    transition: color 160ms ease, box-shadow 180ms ease;
  }
  .order-handle:hover:not([aria-disabled='true']), .order-handle.held { color: var(--text); box-shadow: var(--inset-sm); }
  .order-handle:active, .order-handle.held { cursor: grabbing; }
  .order-handle[aria-disabled='true'] { box-shadow: none; color: var(--muted); cursor: default; }
  .order-handle:focus-visible { outline: 2px solid var(--focus, var(--accent)); outline-offset: 2px; }
  .layout-list .order-handle { inset-block-start: 50%; transform: translateY(-50%); }
  .order-source :global(.project-card) { box-shadow: var(--inset); }
  .order-before::after, .order-after::after {
    content: ''; position: absolute; pointer-events: none; z-index: 12;
    inset-block: 10px; width: 3px; border-radius: 8px; background: var(--accent);
  }
  .order-before::after { inset-inline-start: -6px; }
  .order-after::after { inset-inline-end: -6px; }
  .layout-list .order-before::after, .layout-list .order-after::after { inset-inline: 10px; height: 3px; width: auto; }
  .layout-list .order-before::after { inset-block: -6px auto; }
  .layout-list .order-after::after { inset-block: auto -6px; }
  @media (prefers-reduced-motion: reduce) { .order-handle { transition: none; } }
  @media (forced-colors: active) {
    .order-rail, .order-handle { border: 1px solid ButtonText; }
    .order-before::after, .order-after::after { forced-color-adjust: none; background: Highlight; }
  }
  .library-pages span { margin-right: auto; }
  .index-hint { text-align: center; padding: 28px; color: var(--muted); }
  .heading-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }
  .selection-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 24px 6px;
    color: var(--muted);
    font-size: 12px;
    border-top: 0;
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
    padding: 5px;
  }
  .layout-switcher button {
    padding: 6px 12px;
    font-size: 12px;
    border-radius: 15px;
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
    border-radius: 18px;
    border: 1px solid var(--hairline, var(--glass-border, var(--border)));
    background: var(--glass-panel, var(--surface));
    backdrop-filter: var(--glass-filter, none);
    box-shadow: var(--raise, 0 16px 48px rgba(14, 32, 57, 0.2)), 0 18px 40px rgba(35, 32, 29, 0.16);
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
    border-bottom: 1px solid var(--hairline, var(--border-soft));
    margin-bottom: 4px;
  }
  .resource-menu button {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border-radius: 12px;
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
    background: var(--hairline, var(--border-soft));
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
