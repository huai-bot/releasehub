<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { dispatch, watchReader, type ReaderBounds } from '../lib/bridge';
  import { readableError, safeGithubUrl } from '../lib/ui';
  import type { Theme } from '../lib/types';
  import Icon from './Icon.svelte';

  let { url, theme, preview, onclose }: { url: string; theme: Theme; preview: boolean; onclose: () => void } = $props();
  let dialog: HTMLDialogElement;
  let viewport: HTMLDivElement;
  let title = $state('GitHub 原文');
  let loading = $state(true);
  let error = $state('');
  let notice = $state('');
  let externalUrl = $state('');
  const id = crypto.randomUUID();
  let downloadUrl = $state('');
  let queueing = $state(false);

  function attachmentName(address: string): string {
    try { return decodeURIComponent(new URL(address).pathname.split('/').at(-1) ?? '') || '发行附件'; }
    catch { return '发行附件'; }
  }
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
    try { await dispatch('reader_open', { id, url, theme, bounds: bounds() }); opened = true; }
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
    if (!downloadUrl || queueing) return;
    const selected = downloadUrl;
    queueing = true;
    notice = '正在核对 GitHub 发行附件…';
    try {
      const record = await dispatch('download_release_link', { url: selected });
      if (!disposed && downloadUrl === selected) {
        downloadUrl = '';
        notice = record.status === 'completed' ? `${record.fileName} 已在下载中心。` : `${record.fileName} 已加入下载队列，可在下载中心查看。`;
      }
    } catch (reason) { if (!disposed && downloadUrl === selected) notice = readableError(reason); }
    finally { queueing = false; }
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
          if (event.phase === 'download') {
            downloadUrl = safeGithubUrl(event.value) || ''; externalUrl = downloadUrl;
            notice = downloadUrl ? `发行附件：${attachmentName(downloadUrl)}` : '此下载不是可识别的 GitHub 发行附件，请在浏览器中继续。';
          }
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
    <button class="reader-key" onclick={() => void leave()}><Icon name="chevronRight" class="back" size={16} />返回</button>
    <strong title={title}>{title}</strong>
    {#if loading}<span class="reader-light" role="status" aria-label="正在加载"></span>{/if}
    <button class="reader-key" disabled={preview} onclick={() => void external()}><Icon name="external" size={15} />在浏览器打开</button>
  </header>
  {#if notice}<div class="reader-notice"><span role="status">{notice}</span>{#if downloadUrl}<button class="reader-key" disabled={queueing} onclick={() => void queueAsset()}><Icon name={queueing ? 'loader' : 'download'} class={queueing ? 'spinning' : ''} size={14} />{queueing ? '正在核对' : '在追新下载'}</button>{/if}</div>{/if}
  <div class="reader-body" bind:this={viewport} aria-busy={loading}>
    {#if error}<div class="empty-state"><p role="alert">{error}</p><button class="reader-key" onclick={() => { opening = open(); }}>重试</button></div>
    {:else if preview}<div class="empty-state"><p>桌面版会在这里显示 GitHub 原文。</p></div>{/if}
  </div>
</dialog>

<style>
  dialog.reader { position:fixed; inset:0; margin:auto; width:calc(100vw - 32px); max-width:1440px; height:calc(100vh - 32px); max-height:none; padding:0; overflow:hidden; border:none; border-radius:var(--r-card); background:var(--neu-bg); color:var(--text); font-variant-numeric:tabular-nums; }
  .reader[open] { display:flex; flex-direction:column; }
  .reader-bar { display:flex; align-items:center; gap:20px; flex:none; padding:16px 24px; }
  .reader-bar strong { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--heading); font-size:15px; }
  .reader-key { flex:none; display:inline-flex; align-items:center; justify-content:center; gap:7px; min-height:36px; padding:8px 15px; border:0; border-radius:var(--r-key); background:var(--neu-bg); color:var(--text); font:600 13px/1.5 var(--font-ui); box-shadow:var(--raise); transition:transform 200ms var(--spring-soft),box-shadow 160ms ease,color 160ms ease; }
  .reader-key:hover:not(:disabled) { transform:translateY(-1px); }
  .reader-key:active:not(:disabled) { transform:translateY(1px); box-shadow:var(--inset); color:var(--cobalt); transition-duration:120ms; }
  .reader-key:focus-visible { outline:2px solid var(--cobalt); outline-offset:3px; }
  .reader-key:disabled { color:var(--muted); box-shadow:none; }
  .reader-light { width:8px; height:8px; flex:none; border-radius:50%; background:var(--cobalt); }
  .reader-bar :global(.back) { transform:rotate(180deg); }
  .reader-notice { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:4px 24px 14px; font-size:13px; color:var(--subtle); }
  .reader-notice span { overflow-wrap:anywhere; }
  .reader-notice button { flex:none; }
  .reader-body { flex:1; min-height:0; margin:0 16px 18px; background:var(--neu-bg); border-radius:var(--r-card); }
  .reader-body .empty-state { height:100%; }
  .reader-body [role="alert"] { color:var(--danger); }
  @media (prefers-reduced-motion:reduce) { .reader-key:is(:hover,:active):not(:disabled) { transform:none; } }
  @media (forced-colors:active) { .reader-key, dialog.reader { border:1px solid ButtonText; } .reader-light { background:Highlight; } }
</style>
