<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { dispatch, watchReader, type ReaderBounds } from '../lib/bridge';
  import { readableError } from '../lib/ui';

  const home = 'https://flingtrainer.com/';
  let { preview, active = true }: { preview: boolean; active?: boolean } = $props();
  let viewport: HTMLDivElement;
  let mounted = $state(false);
  let opened = $state(false);
  let loading = $state(false);
  let saving = $state(false);
  let canGoBack = $state(false);
  let query = $state('');
  let pending = $state('');
  let saved = $state<{ path: string; fileName: string } | null>(null);
  let notice = $state('');
  let error = $state('');
  let liveId = '';
  let disposed = false;
  let lifecycle = Promise.resolve();
  let loadTimer: ReturnType<typeof setTimeout> | undefined;
  let history: string[] = [];

  function bounds(): ReaderBounds {
    const rect = viewport.getBoundingClientRect();
    return { x: Math.round(rect.left), y: Math.round(rect.top), width: Math.round(rect.width), height: Math.round(rect.height) };
  }

  function waitForPage() {
    clearTimeout(loadTimer);
    loading = true;
    loadTimer = setTimeout(() => {
      loading = false;
      notice = '官网响应较慢，可重试或用浏览器打开。';
    }, 20000);
  }

  function syncView(wantView: boolean) {
    lifecycle = lifecycle.then(async () => {
      if (wantView && !disposed && active && !preview) {
        if (liveId) return;
        await tick();
        if (disposed || !active || preview) return;
        const id = crypto.randomUUID();
        liveId = id;
        error = ''; notice = ''; pending = ''; saved = null; saving = false;
        history = []; canGoBack = false;
        waitForPage();
        try {
          await dispatch('trainer_open', { id, bounds: bounds() });
          opened = liveId === id;
        } catch (reason) {
          if (liveId === id) liveId = '';
          if (!disposed) { error = readableError(reason); loading = false; }
          clearTimeout(loadTimer);
        }
      } else if (liveId) {
        const id = liveId;
        clearTimeout(loadTimer);
        if (!disposed) { opened = false; loading = false; saving = false; pending = ''; }
        try { await dispatch('trainer_close', { id }); if (liveId === id) liveId = ''; }
        catch (reason) { if (!disposed) error = readableError(reason); }
      }
    });
  }

  $effect(() => { if (mounted) syncView(active && !preview); });

  async function navigate(location: 'home' | 'catalog' | 'search' | 'back') {
    if (!opened || !liveId || !active) return;
    const prior = history.slice();
    if (location === 'back') {
      if (!canGoBack) return;
      history.pop(); canGoBack = history.length > 1;
    }
    notice = '';
    try { await dispatch('trainer_navigate', { id: liveId, location, ...(location === 'search' ? { query } : {}) }); }
    catch (reason) { history = prior; canGoBack = prior.length > 1; notice = readableError(reason); }
  }

  async function save() {
    if (!liveId || !pending || saving) return;
    const id = liveId;
    saving = true;
    notice = '正在确认官网附件并选择保存位置…';
    try {
      const result = await dispatch('trainer_save', { id, url: pending });
      if (disposed || liveId !== id) return;
      if (result) { saved = result; pending = ''; notice = ''; }
      else { notice = '未选择保存位置，未下载文件。'; }
    } catch (reason) { if (!disposed && liveId === id) notice = readableError(reason); }
    finally { if (!disposed && liveId === id) saving = false; }
  }

  async function nativeAction(action: 'trainer_external' | 'trainer_reveal') {
    if (!liveId) return;
    try { await dispatch(action, { id: liveId }); }
    catch (reason) { notice = readableError(reason); }
  }

  onMount(() => {
    if (preview) return;
    let unlisten: (() => void) | undefined;
    let frame = 0;
    const resize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (!liveId || !opened || !active || disposed) return;
        const id = liveId;
        const rect = bounds();
        if (rect.width < 1 || rect.height < 1 || rect.x < 0 || rect.y < 0) return;
        void dispatch('trainer_resize', { id, bounds: rect }).catch(reason => {
          if (liveId === id && active && !disposed) notice = readableError(reason);
        });
      });
    };
    const observer = new ResizeObserver(resize);
    observer.observe(viewport);
    window.addEventListener('resize', resize);
    void watchReader(event => {
      if (!active || disposed || event.id !== liveId) return;
      if (event.phase === 'loading') {
        if (!saving) { pending = ''; notice = ''; }
        waitForPage();
      }
      if (event.phase === 'ready') {
        if (history.at(-1) !== event.value) history = [...history.slice(-63), event.value];
        canGoBack = history.length > 1;
        clearTimeout(loadTimer); loading = false;
      }
      if (event.phase === 'blocked' && !saving) {
        notice = '已拦截外部或不受支持的链接。如需访问第三方内容，请在浏览器中打开官网。';
      }
      if (event.phase === 'download' && !saving) {
        pending = event.value; saved = null; notice = '';
      }
      if (event.phase === 'closed') {
        liveId = ''; opened = false; saving = false; pending = ''; saved = null;
        clearTimeout(loadTimer); loading = false;
        error = '官网视图已关闭，请重新打开。';
      }
    }).then(release => {
      if (disposed) release(); else { unlisten = release; mounted = true; }
    }, reason => { if (!disposed) error = readableError(reason); });
    return () => {
      disposed = true;
      clearTimeout(loadTimer); observer.disconnect(); cancelAnimationFrame(frame);
      window.removeEventListener('resize', resize); unlisten?.(); syncView(false);
    };
  });
</script>

<section class="trainer-page" aria-labelledby="trainer-heading">
  <div class="page-heading">
    <div><h1 id="trainer-heading">风灵月影</h1><p>浏览官网目录、搜索游戏和保存官方附件。</p></div>
    {#if opened}
      <button class="button secondary" onclick={() => void nativeAction('trainer_external')}><Icon name="external" size={15} />在浏览器打开</button>
    {:else}
      <a class="button secondary" href={home} target="_blank" rel="noopener noreferrer"><Icon name="external" size={15} />在浏览器打开</a>
    {/if}
  </div>

  <div class="trainer-toolbar" aria-label="官网导航">
    <button class="icon-button" aria-label="返回上一页" title="返回上一页" disabled={!opened || !canGoBack || loading} onclick={() => void navigate('back')}><Icon name="chevronRight" class="trainer-back" size={18} /></button>
    <button class="button secondary" disabled={!opened} onclick={() => void navigate('home')}>首页</button>
    <button class="button secondary" disabled={!opened} onclick={() => void navigate('catalog')}>全部修改器</button>
    <form role="search" aria-label="风灵月影官网搜索" onsubmit={event => { event.preventDefault(); void navigate('search'); }}>
      <label class="global-search"><Icon name="search" size={16} /><input aria-label="游戏名称" type="search" placeholder="搜索游戏名称" maxlength="160" bind:value={query} disabled={!opened} /></label>
      <button class="button secondary" type="submit" disabled={!opened || !query.trim()}>搜索</button>
    </form>
  </div>

  <div class="trainer-notice">
    <div class="trainer-status" role="status">
      <Icon name={loading ? 'loader' : saved ? 'success' : 'shield'} class={loading ? 'spinning' : ''} size={16} />
      {#if notice}<span>{notice}</span>
      {:else if pending}<span>已选中官网附件。确认后选择保存位置；追新不会运行该文件。</span>
      {:else if saved}<span title={saved.path}>已保存：{saved.fileName}</span>
      {:else if loading}<span>正在加载 flingtrainer.com…</span>
      {:else}<span>flingtrainer.com · 只保存官网附件，不会自动运行。</span>{/if}
    </div>
    {#if pending}
      <button class="button primary" disabled={saving || !opened} onclick={() => void save()}><Icon name={saving ? 'loader' : 'download'} class={saving ? 'spinning' : ''} size={15} />{saving ? '正在保存' : '另存为…'}</button>
    {:else if saved}
      <button class="button secondary" disabled={!opened} onclick={() => void nativeAction('trainer_reveal')}><Icon name="folder" size={15} />在文件夹中显示</button>
    {/if}
  </div>

  <div class="trainer-viewport" bind:this={viewport} aria-busy={loading} aria-label="风灵月影官网内容">
    {#if preview}
      <div class="empty-state"><Icon name="gamepad" size={26} /><h3>在桌面版浏览官网</h3><p>浏览器预览无法接入本地官网视图和附件保存。</p></div>
    {:else if error}
      <div class="empty-state"><Icon name="alert" size={24} /><h3>官网未能打开</h3><p role="alert">{error}</p><button class="button secondary" disabled={!active} onclick={() => syncView(true)}><Icon name="refresh" size={15} />重新打开</button></div>
    {:else if !opened && active}
      <div class="empty-state"><Icon name="loader" class="spinning" size={24} /><p>正在启动官网视图…</p></div>
    {/if}
  </div>
</section>

<style>
  .trainer-page { display:flex; flex-direction:column; gap:12px; height:100%; min-height:0; }
  .trainer-page .page-heading { margin-bottom:3px; }
  .trainer-page .page-heading .button { flex:none; }
  .trainer-toolbar { display:flex; align-items:center; flex-wrap:wrap; gap:8px; }
  .trainer-toolbar > .icon-button { min-width:34px; min-height:32px; }
  .trainer-toolbar :global(.trainer-back) { transform:rotate(180deg); }
  .trainer-toolbar form { display:flex; flex:1 1 220px; align-items:center; justify-content:flex-end; gap:8px; min-width:0; }
  .trainer-toolbar .global-search { flex:1 1 160px; max-width:340px; min-width:0; }
  .trainer-notice { display:flex; align-items:center; justify-content:space-between; gap:12px; min-height:38px; }
  .trainer-status { display:flex; align-items:center; gap:7px; font-size:12px; line-height:1.5; color:var(--subtle); }
  .trainer-status :global(svg), .trainer-notice .button { flex:none; }
  .trainer-status span { overflow-wrap:anywhere; }
  .trainer-viewport { flex:1; min-height:0; border:1px solid var(--border); background:var(--surface-soft); }
  .trainer-viewport .empty-state { display:flex; height:100%; justify-content:center; }
  .trainer-viewport [role="alert"] { color:var(--danger); }
</style>
