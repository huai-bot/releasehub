<script lang="ts">
  import { hasUnreadUpdate, type Page } from '../lib/ui';
  import type { Snapshot, WorkspacePage } from '../lib/types';
  import { normalizeNavigation, moveNavigation, workspacePages } from '../lib/navigation';
  import Icon from './Icon.svelte';
  import BrandMark from './BrandMark.svelte';
  let { page, snapshot, preview, busy = false, onnavigate, onadd, ondata, onreorder }: {
    page: Page; snapshot: Snapshot; preview: boolean; busy?: boolean;
    onnavigate: (page: Page) => void; onadd: () => void; ondata: () => void;
    onreorder: (order: WorkspacePage[]) => Promise<boolean>;
  } = $props();
  const updateCount = $derived(snapshot.projects.filter(project => hasUnreadUpdate(project, snapshot.readUpdateTags)).length);
  const downloadCount = $derived(snapshot.downloads.filter((download) => ['queued', 'downloading'].includes(download.status)).length);
  const order = $derived(normalizeNavigation(snapshot.settings.navigationOrder));
  const pages = $derived(order.map((id) => workspacePages.find((item) => item.id === id)!));
  let dragging = $state<WorkspacePage | null>(null);
  let dropTarget = $state<WorkspacePage | null>(null);
  let sorting = $state(false);
  let announcement = $state('');
  async function reorder(from: WorkspacePage, to: WorkspacePage) {
    if (busy || sorting || from === to) return;
    sorting = true;
    try { if (await onreorder(moveNavigation(order, from, to))) announcement = '工作空间顺序已保存'; }
    finally { sorting = false; dragging = null; dropTarget = null; }
  }
  function startDrag(event: DragEvent, id: WorkspacePage) {
    if (busy || sorting || !event.dataTransfer) { event.preventDefault(); return; }
    dragging = id; event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', id);
  }
  function drop(event: DragEvent, id: WorkspacePage) {
    event.preventDefault(); const from = dragging; dragging = null; dropTarget = null;
    if (from) void reorder(from, id);
  }
  function keyboardOrder(event: KeyboardEvent, id: WorkspacePage) {
    if (!event.altKey || !['ArrowUp', 'ArrowDown'].includes(event.key)) return;
    event.preventDefault(); const index = order.indexOf(id) + (event.key === 'ArrowUp' ? -1 : 1);
    if (index >= 0 && index < order.length) void reorder(id, order[index]);
  }
</script>

<aside class="sidebar">
  <button class="brand" onclick={() => onnavigate('home')} aria-label="ReleaseHub 首页"><BrandMark /><strong>ReleaseHub</strong></button>
  <nav aria-label="主导航">
    {#each pages as item (item.id)}
      <button class:active={page === item.id} class:dragging={dragging === item.id} class:drop-target={dropTarget === item.id && dragging !== item.id} class="nav-item" aria-current={page === item.id ? 'page' : undefined} aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown" draggable={!busy && !sorting} ondragstart={(event) => startDrag(event, item.id)} ondragend={() => { dragging = null; dropTarget = null; }} ondragover={(event) => { if (dragging) { event.preventDefault(); dropTarget = item.id; } }} ondragleave={() => { if (dropTarget === item.id) dropTarget = null; }} ondrop={(event) => drop(event, item.id)} onkeydown={(event) => keyboardOrder(event, item.id)} onclick={() => { if (!dragging) onnavigate(item.id); }}>
        <Icon name={item.icon} size={19} /><span>{item.label}</span>
        {#if item.id === 'updates' && updateCount}<span class="nav-count">{updateCount}</span>{/if}
        {#if item.id === 'downloads' && downloadCount}<span class="nav-dot" aria-label={`${downloadCount} 项进行中`}></span>{/if}
        <i class="nav-led" aria-hidden="true"></i>
      </button>
    {/each}
  </nav>
  <span class="sr-only" aria-live="polite">{announcement}</span>
  <button class="sidebar-add" onclick={onadd}><Icon name="plus" size={17} />添加资源</button>
  <div class="sidebar-spacer"></div>
  <button class:active={page === 'settings'} class="nav-item settings-nav" aria-current={page === 'settings' ? 'page' : undefined} onclick={() => onnavigate('settings')}><Icon name="settings" size={19} /><span>设置</span><i class="nav-led" aria-hidden="true"></i></button>
  <div class="sidebar-footer"><button class="storage-button" onclick={ondata} title="打开数据目录"><span class="status-dot"></span>{preview ? '界面预览' : snapshot.runtime.portable ? '便携工作空间' : '本机工作空间'}<Icon name="chevronRight" size={13} /></button><span>v{snapshot.runtime.appVersion} <span class="footer-dot">·</span> Windows x64</span></div>
</aside>

<style>
  .nav-item.dragging { opacity: .45; }
  .nav-item.drop-target { box-shadow: inset 0 2px 0 var(--accent); background: var(--accent-soft); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
</style>
