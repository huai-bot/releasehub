<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { dispatch, watchReader, type ReaderBounds } from '../lib/bridge';
  import { readableError, safeGithubUrl, trackedAsset } from '../lib/ui';
  import type { Project } from '../lib/types';
  import Icon from './Icon.svelte';

  let { url, preview, projects, ondownload, onclose }: { url: string; preview: boolean; projects: Project[]; ondownload: (project: Project) => void; onclose: () => void } = $props();
  let dialog: HTMLDialogElement;
  let viewport: HTMLDivElement;
  let title = $state('GitHub 原文');
  let loading = $state(true);
  let error = $state('');
  let notice = $state('');
  let externalUrl = $state('');
  const id = crypto.randomUUID();
  let downloadUrl = $state('');
  const download = $derived(downloadUrl ? trackedAsset(projects, downloadUrl) : null);
  let disposed = false;
  let opened = false;
  let opening: Promise<void> = Promise.resolve();
  let timeout: ReturnType<typeof setTimeout>;
  let closing = false;
  let returnFocus: HTMLElement | null = null;

  function bounds(): ReaderBounds {
    const rect = viewport.getBoundingClientRect();
    return { x: Math.round(rect.left), y: Math.round(rect.top), width: Math.round(rect.width), height: Math.round(rect.height) };
  }
  function waitForPage() {
    clearTimeout(timeout); loading = true;
    timeout = setTimeout(() => { if (!disposed && loading) { loading = false; notice = '网页响应较慢，可在浏览器中继续。'; } }, 20000);
  }
  async function open() {
    error = ''; notice = ''; externalUrl = ''; downloadUrl = ''; waitForPage();
    if (preview) { loading = false; return; }
    try { await dispatch('reader_open', { id, url, bounds: bounds() }); opened = true; }
    catch (reason) { clearTimeout(timeout); loading = false; error = readableError(reason); }
  }
  async function leave() {
    if (closing) return;
    closing = true;
    await opening;
    try {
      if (opened) await dispatch('reader_close', { id });
      opened = false; onclose();
      return true;
    } catch (reason) { notice = readableError(reason); closing = false; return false; }
  }
  async function queueAsset() {
    if (!download) return;
    const choice = { ...download.project, selectedAssetId: download.asset.id };
    if (await leave()) ondownload(choice);
  }
  async function external() {
    if (preview) return;
    try {
      if (externalUrl || !opened) await dispatch('open_url', { url: externalUrl || url });
      else await dispatch('reader_external', { id });
    } catch (reason) { notice = readableError(reason); }
  }
  onMount(() => {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    let unlisten: (() => void) | undefined;
    let observer: ResizeObserver | undefined;
    let frame = 0;
    const resize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => { if (opened && !disposed && !closing) void dispatch('reader_resize', { id, bounds: bounds() }).catch(reason => notice = readableError(reason)); });
    };
    opening = (async () => {
      if (!preview) {
        unlisten = await watchReader(event => {
          if (disposed || event.id !== id) return;
          if (event.phase === 'title') title = event.value || 'GitHub 原文';
          if (event.phase === 'loading') { notice = ''; externalUrl = ''; downloadUrl = ''; waitForPage(); }
          if (event.phase === 'ready') { clearTimeout(timeout); loading = false; }
          if (event.phase === 'blocked') { downloadUrl = ''; externalUrl = safeGithubUrl(event.value) || ''; notice = externalUrl ? '此账号操作请在浏览器中继续。' : '此链接不属于 GitHub，可从浏览器中的原页继续。'; }
          if (event.phase === 'download') { downloadUrl = safeGithubUrl(event.value) || ''; externalUrl = downloadUrl; notice = trackedAsset(projects, downloadUrl)?.asset.name ?? '此文件不在已订阅的发行文件中，可在浏览器中下载。'; }
          if (event.phase === 'closed') { opened = false; onclose(); }
        });
      }
      if (disposed || closing) return;
      await tick(); await open();
      if (!disposed && !closing && opened) {
        observer = new ResizeObserver(resize); observer.observe(viewport); window.addEventListener('resize',resize); resize();
      }
    })().catch(reason => { error = readableError(reason); loading = false; });
    return () => {
      disposed = true; clearTimeout(timeout); observer?.disconnect(); cancelAnimationFrame(frame); window.removeEventListener('resize',resize);
      void opening.finally(() => { unlisten?.(); if (opened) void dispatch('reader_close', { id }).catch(() => {}); });
      dialog.close(); if (returnFocus?.isConnected) returnFocus.focus({preventScroll:true});
    };
  });
</script>

<dialog class="reader" bind:this={dialog} aria-label="GitHub 原文" oncancel={event => { event.preventDefault(); void leave(); }}>
  <header class="reader-bar">
    <button class="button secondary small" onclick={() => void leave()}><Icon name="chevronRight" class="back" size={16} />返回</button>
    <strong title={title}>{title}</strong>
    {#if loading}<Icon name="loader" class="spinning" size={16} />{/if}
    <button class="text-button" disabled={preview} onclick={() => void external()}><Icon name="external" size={15} />在浏览器打开</button>
  </header>
  {#if notice}<div class="reader-notice" role="status"><span>{notice}</span>{#if download}<button class="button secondary small" onclick={() => void queueAsset()}><Icon name="download" size={14} />在追新下载</button>{/if}</div>{/if}
  <div class="reader-body" bind:this={viewport} aria-busy={loading}>
    {#if error}<div class="empty-state"><p role="alert">{error}</p><button class="button secondary" onclick={() => { opening = open(); }}>重试</button></div>
    {:else if preview}<div class="empty-state"><p>桌面版会在这里显示 GitHub 原文。</p></div>{/if}
  </div>
</dialog>

<style>
  dialog.reader { position:fixed; inset:0; margin:auto; width:calc(100vw - 32px); max-width:1440px; height:calc(100vh - 32px); max-height:none; padding:0; overflow:hidden; background:var(--surface); color:var(--text); }
  .reader[open] { display:flex; flex-direction:column; }
  .reader-bar { display:flex; align-items:center; gap:16px; flex:none; padding:12px 20px; border-bottom:1px solid var(--border); }
  .reader-bar strong { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--heading); font-size:14px; }
  .reader-bar button { flex:none; }
  .reader-bar :global(.back) { transform:rotate(180deg); }
  .reader-notice { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:7px 20px; font-size:12px; color:var(--subtle); }
  .reader-notice span { overflow-wrap:anywhere; }
  .reader-notice button { flex:none; }
  .reader-body { flex:1; min-height:0; margin:0 8px 8px; }
  .reader-body .empty-state { height:100%; }
</style>
