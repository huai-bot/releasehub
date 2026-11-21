<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { desktopAvailable } from '../lib/bridge';
  import Icon from './Icon.svelte';
  let { title = '工作空间', query = $bindable(''), inputElement = $bindable<HTMLInputElement>(), searchable = true, busy = false, refreshing = false, onsearch = () => {}, onrefresh = () => {}, onerror = () => {} }: {
    title?: string; query?: string; inputElement?: HTMLInputElement; searchable?: boolean;
    busy?: boolean; refreshing?: boolean; onsearch?: () => void; onrefresh?: () => void; onerror?: (error: unknown) => void;
  } = $props();
  const native = desktopAvailable();
  let maximized = $state(false);
  onMount(() => {
    if (!native) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    const update = () => void window.isMaximized().then((value) => { if (!disposed) maximized = value; }).catch(onerror);
    update();
    void window.onResized(update).then((release) => { if (disposed) release(); else unlisten = release; }).catch(onerror);
    return () => { disposed = true; unlisten?.(); };
  });
  async function control(action: 'minimize' | 'maximize' | 'close') {
    if (!native) return;
    try {
      const window = getCurrentWindow();
      if (action === 'minimize') await window.minimize();
      else if (action === 'close') await window.close();
      else { await window.toggleMaximize(); maximized = await window.isMaximized(); }
    } catch (error) { onerror(error); }
  }
</script>

<header class="topbar titlebar" data-tauri-drag-region>
  <div class="breadcrumb" data-tauri-drag-region><strong data-tauri-drag-region>{title}</strong></div>
  {#if searchable}
    <div class="global-search"><Icon name="search" size={16} /><input bind:this={inputElement} bind:value={query} aria-label={title === '下载' ? '搜索下载' : '搜索资源库'} aria-keyshortcuts="Control+K Meta+K" placeholder={title === '下载' ? '搜索下载…' : '搜索资源库…'} oninput={onsearch} />{#if query}<button class="icon-button" aria-label="清空搜索" onclick={() => query = ''}><Icon name="close" size={13} /></button>{:else}<kbd>Ctrl K</kbd>{/if}</div>
    <button class="icon-button topbar-refresh" disabled={busy || refreshing} aria-label="刷新本机状态" title="刷新本机状态" onclick={onrefresh}><Icon name="refresh" class={refreshing ? 'spinning' : ''} size={17} /></button>
  {/if}
  <div class="window-controls" aria-label="窗口操作">
    <button disabled={!native} title="最小化" aria-label="最小化窗口" onclick={() => void control('minimize')}><Icon name="minimize" size={14} strokeWidth={1.4} /></button>
    <button disabled={!native} title={maximized ? '还原窗口' : '最大化'} aria-label={maximized ? '还原窗口' : '最大化窗口'} onclick={() => void control('maximize')}><Icon name={maximized ? 'restore' : 'maximize'} size={maximized ? 13 : 12} strokeWidth={1.3} /></button>
    <button disabled={!native} class="window-close" title="关闭窗口" aria-label="关闭窗口" onclick={() => void control('close')}><Icon name="close" size={17} strokeWidth={1.4} /></button>
  </div>
</header>

<style>
  .titlebar { padding-right: 0; gap: 12px; user-select: none; }
  .breadcrumb { align-self: stretch; min-width: 105px; }
  .global-search { width: clamp(210px, 26vw, 330px); }
  .window-controls { display: flex; align-self: stretch; margin-left: 2px; border-left: 1px solid var(--border); }
  .window-controls button { display: grid; place-items: center; width: 42px; padding: 0; color: var(--subtle); border-radius: 0; }
  .window-controls button:hover:not(:disabled) { background: var(--border); color: var(--heading); }
  .window-controls .window-close:hover:not(:disabled) { background: #c42b1c; color: #fff; }
  .window-controls button:focus-visible { outline-offset: -4px; }
  .window-controls button:disabled { opacity: .55; cursor: default; }
  @media (max-width: 1130px) { .titlebar { gap: 8px; padding-left: 18px; } .global-search { width: 215px; } .breadcrumb { min-width: 56px; } .window-controls button { width: 38px; } }
</style>
