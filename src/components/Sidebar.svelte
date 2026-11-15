<script lang="ts">
  import type { Page } from '../lib/ui';
  import type { Snapshot, WorkspacePage } from '../lib/types';
  import { normalizeNavigation, moveNavigation, workspacePages } from '../lib/navigation';
  import Icon from './Icon.svelte';
  import BrandMark from './BrandMark.svelte';
  let { page, snapshot, preview, busy = false, onnavigate, onadd, ondata, onreorder }: {
    page: Page; snapshot: Snapshot; preview: boolean; busy?: boolean;
    onnavigate: (page: Page) => void; onadd: () => void; ondata: () => void;
    onreorder: (order: WorkspacePage[]) => Promise<boolean>;
  } = $props();
  const updateCount = $derived(snapshot.projects.filter((project) => project.status === 'updateAvailable').length);
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
  <button class="brand" onclick={() => onnavigate('home')} aria-label="ReleaseHub 追新首页"><BrandMark /><span><strong>ReleaseHub</strong><small>追新 · 让更新更从容</small></span></button>
  <div class="nav-group-label">工作空间<span title="拖动页面调整顺序，或聚焦后按 Alt + 上下方向键"><Icon name="grip" size={13} /></span></div>
  <nav aria-label="主导航" aria-describedby="navigation-help">
    {#each pages as item (item.id)}
      <button class:active={page === item.id} class:dragging={dragging === item.id} class:drop-target={dropTarget === item.id && dragging !== item.id} class="nav-item" aria-current={page === item.id ? 'page' : undefined} draggable={!busy && !sorting} ondragstart={(event) => startDrag(event, item.id)} ondragend={() => { dragging = null; dropTarget = null; }} ondragover={(event) => { if (dragging) { event.preventDefault(); dropTarget = item.id; } }} ondragleave={() => { if (dropTarget === item.id) dropTarget = null; }} ondrop={(event) => drop(event, item.id)} onkeydown={(event) => keyboardOrder(event, item.id)} onclick={() => { if (!dragging) onnavigate(item.id); }}>
        <Icon name={item.icon} size={19} /><span>{item.label}</span>
        {#if item.id === 'updates' && updateCount}<span class="nav-count">{updateCount}</span>{/if}
        {#if item.id === 'downloads' && downloadCount}<span class="nav-dot" aria-label={`${downloadCount} 项进行中`}></span>{/if}
      </button>
    {/each}
  </nav>
  <span id="navigation-help" class="sr-only">拖动页面调整顺序，也可聚焦页面后按 Alt 加上下方向键。</span><span class="sr-only" aria-live="polite">{announcement}</span>
  <button class="sidebar-add" onclick={onadd}><Icon name="plus" size={17} />添加软件</button>
  <div class="sidebar-spacer"></div>
  <div class="workspace-note"><span class="note-icon"><Icon name="activity" size={18} /></span><strong>软件动态，一处掌握</strong><p>订阅发行动态，集中管理更新。</p></div>
  <button class:active={page === 'settings'} class="nav-item settings-nav" aria-current={page === 'settings' ? 'page' : undefined} onclick={() => onnavigate('settings')}><Icon name="settings" size={19} /><span>设置</span></button>
  <div class="sidebar-footer"><button class="storage-button" onclick={ondata} title="打开数据目录"><span class="status-dot"></span>{preview ? '界面预览' : snapshot.runtime.portable ? '便携工作空间' : '本机工作空间'}<Icon name="chevronRight" size={13} /></button><span>v{snapshot.runtime.appVersion} <span class="footer-dot">·</span> Windows x64</span></div>
</aside>

<style>
  .nav-group-label { display: flex; align-items: center; justify-content: space-between; }
  .nav-group-label > span { display: flex; color: var(--muted); }
  .nav-item.dragging { opacity: .45; }
  .nav-item.drop-target { box-shadow: inset 0 2px 0 var(--accent); background: var(--accent-soft); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
</style>
