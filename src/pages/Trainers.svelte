<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { getTrainerDetail, listTrainers, openTrainerOriginal, revealSavedTrainer, saveTrainerAttachment, sendTrainerToDownloader, type TrainerAttachment, type TrainerCatalogMode, type TrainerDetail, type TrainerEntry, type TrainerListPage } from '../lib/trainer-catalog';
  import { readableError } from '../lib/ui';
  import { handoffMessage, type HandoffStatus } from '../lib/external-handoff';

  let { preview, active = true, externalDownloaderName = null }: { preview: boolean; active?: boolean; externalDownloaderName?: string | null } = $props();
  const home = 'https://flingtrainer.com/';
  let surface: HTMLElement;
  let detailHeading = $state<HTMLHeadingElement>();
  let queryInput: HTMLInputElement;
  let mode = $state<TrainerCatalogMode>('latest');
  let browseMode: 'latest' | 'all' = 'latest';
  let draft = $state('');
  let query = $state('');
  let page = $state(1);
  let listing = $state.raw<TrainerListPage | null>(null);
  let listBusy = $state(true);
  let listError = $state('');
  let entry = $state.raw<TrainerEntry | null>(null);
  let detail = $state.raw<TrainerDetail | null>(null);
  let detailBusy = $state(false);
  let detailError = $state('');
  let actionError = $state('');
  let saving = $state('');
  let sending = $state('');
  let handoffs = $state.raw<ReadonlyMap<string, HandoffStatus>>(new Map());
  const actionBusy = $derived(!!saving || !!sending);
  function receiptKey(name: string, file: TrainerAttachment) { return JSON.stringify([name, file.url]); }
  let saveNote = $state('');
  let savedPath = $state('');
  let showAllFiles = $state(false);
  let listSeq = 0, detailSeq = 0, viewSeq = 0, listScroll = 0;
  const listName = $derived(mode === 'search' ? '搜索结果' : mode === 'latest' ? '最近更新' : '全部修改器');
  const browseName = $derived(mode === 'search' ? '搜索结果' : '目录');
  const files = $derived(detail?.attachments ?? []);

  function dateLabel(value: string | null) { return value?.match(/^\d{4}-\d{2}-\d{2}/)?.[0].replaceAll('-', '.') ?? '—'; }
  function gameName(title: string) { return title.replace(/\s+trainer\s*$/i, '').trim() || title; }
  function scrollArea() { return surface?.closest<HTMLElement>('.page-content'); }
  function clearActions() { actionError = ''; saveNote = ''; savedPath = ''; showAllFiles = false; }

  async function loadList(to: TrainerCatalogMode, term: string, at: number, forceRefresh = false) {
    const id = ++listSeq;
    const same = mode === to && query === term && page === at;
    ++viewSeq; ++detailSeq; entry = null; detail = null; clearActions();
    if (!same) { listing = null; if (active && scrollArea()) scrollArea()!.scrollTop = 0; }
    mode = to; query = term; page = at; listError = ''; listBusy = true;
    try {
      const next = preview && import.meta.env.DEV ? (await import('../lib/trainer-preview')).previewList(to, term) : await listTrainers(to, term, at, forceRefresh);
      if (id !== listSeq) return;
      listing = next; page = next.page;
    } catch (error) { if (id === listSeq) listError = readableError(error); }
    finally { if (id === listSeq) listBusy = false; }
  }
  function browse(to: 'latest' | 'all') { if (!active) return; browseMode = to; draft = ''; void loadList(to, '', 1); }
  function search() {
    if (!active) return;
    const term = draft.trim(); draft = term;
    if (!term) { if (mode === 'search') browse(browseMode); return; }
    void loadList('search', term, 1);
  }
  function clearQuery() { draft = ''; if (mode === 'search') browse(browseMode); queryInput.focus(); }

  async function openEntry(item: TrainerEntry, retry = false) {
    if (!active) return;
    if (!retry) { listScroll = scrollArea()?.scrollTop ?? 0; ++viewSeq; }
    const id = ++detailSeq;
    entry = item; if (!retry) detail = null; detailError = ''; detailBusy = true; clearActions();
    await tick();
    if (id !== detailSeq) return;
    if (active && !retry) { detailHeading?.focus({ preventScroll: true }); if (scrollArea()) scrollArea()!.scrollTop = 0; }
    try {
      const next = preview && import.meta.env.DEV ? (await import('../lib/trainer-preview')).previewDetail(item.url) : await getTrainerDetail(item.url, retry);
      if (id === detailSeq) detail = next;
    } catch (error) { if (id === detailSeq) detailError = readableError(error); }
    finally { if (id === detailSeq) detailBusy = false; }
  }
  async function back() {
    const selectedUrl = entry?.url;
    ++detailSeq; ++viewSeq; detailBusy = false; entry = null; detail = null; detailError = ''; clearActions();
    await tick();
    if (!active || entry) return;
    const button = [...surface.querySelectorAll<HTMLButtonElement>('.game-row')].find(row => row.dataset.url === selectedUrl);
    button?.focus({ preventScroll: true }); if (scrollArea()) scrollArea()!.scrollTop = listScroll;
  }
  async function original() {
    if (!active) return;
    const id = viewSeq; actionError = '';
    const url = entry?.url ?? home;
    if (preview) { window.open(home, '_blank', 'noopener,noreferrer'); return; }
    try { await openTrainerOriginal(url); } catch (error) { if (id === viewSeq) actionError = readableError(error); }
  }
  async function reveal() {
    if (!active || preview || !savedPath) return;
    const id = viewSeq; actionError = '';
    try { await revealSavedTrainer(savedPath); } catch (error) { if (id === viewSeq) actionError = readableError(error); }
  }
  async function send(file: TrainerAttachment) {
    const name = externalDownloaderName;
    if (!active || !detail || preview || actionBusy || !name) return;
    const key = receiptKey(name, file);
    if (handoffs.has(key)) return;
    const id = viewSeq;
    sending = file.url; actionError = ''; saveNote = ''; savedPath = '';
    try {
      const { status } = await sendTrainerToDownloader(detail.url, file.url);
      handoffs = new Map(handoffs).set(key, status);
      if (id === viewSeq) saveNote = handoffMessage(name, status);
    } catch (error) { if (id === viewSeq) actionError = readableError(error); }
    finally { sending = ''; }
  }
  async function save(file: TrainerAttachment) {
    if (!active || !detail || preview || actionBusy) return;
    const id = viewSeq;
    actionError = ''; saveNote = ''; savedPath = ''; saving = file.url;
    try {
      const result = await saveTrainerAttachment(detail.url, file.url);
      if (id !== viewSeq) return;
      if (result.status === 'saved') { savedPath = result.path; saveNote = '已保存至'; }
      else saveNote = '已取消，未保存文件。';
    } catch (error) { if (id === viewSeq) actionError = readableError(error); }
    finally { saving = ''; }
  }
  onMount(() => { void loadList('latest', '', 1); return () => { ++listSeq; ++detailSeq; ++viewSeq; }; });
</script>

<section class="trainer-page" bind:this={surface} aria-labelledby="trainer-heading">
  <header class="page-heading">
    <div><h1 id="trainer-heading">风灵月影</h1><p>浏览 FLiNG 官方目录，查找游戏修改器和版本附件。</p></div>
    <button class="button secondary original" onclick={() => void original()}><Icon name="external" size={15} />{entry && !preview ? '官网原文' : '打开官网'}</button>
  </header>
  <div class="trainer-toolbar">
    <div class="segmented-tabs trainer-tabs" role="group" aria-label="官网目录">
      <button class:active={mode === 'latest'} aria-pressed={mode === 'latest'} onclick={() => browse('latest')}><Icon name="clock" size={15} />最近更新</button>
      <button class:active={mode === 'all'} aria-pressed={mode === 'all'} onclick={() => browse('all')}><Icon name="readme" size={15} />全部修改器</button>
    </div>
    <form role="search" aria-label="风灵月影官网搜索" onsubmit={e => { e.preventDefault(); search(); }}>
      <div class="global-search trainer-search"><Icon name="search" size={16} />
        <input bind:this={queryInput} bind:value={draft} aria-label="游戏名称" maxlength="160" placeholder="搜索游戏名称" autocomplete="off" spellcheck="false" />
        {#if draft || mode === 'search'}<button type="button" class="icon-button" aria-label="清除搜索" onclick={clearQuery}><Icon name="close" size={14} /></button>{/if}
      </div>
      <button type="submit" class="button secondary" disabled={!draft.trim() && mode !== 'search'}>搜索</button>
    </form>
  </div>

  {#if entry}
    <div class="detail-nav"><button class="back-link" onclick={() => void back()}><Icon name="arrowRight" size={15} class="reverse" />返回{browseName}</button><button class="icon-button" aria-label="刷新详情" title="刷新详情" disabled={detailBusy || actionBusy} onclick={() => entry && void openEntry(entry, true)}><Icon name={detailBusy ? 'loader' : 'refresh'} class={detailBusy ? 'spinning' : ''} size={15} /></button></div>
    <article aria-labelledby="game-heading" aria-busy={detailBusy}>
      <header class="game-heading"><h2 id="game-heading" bind:this={detailHeading} tabindex="-1">{gameName(entry.title)}</h2>
        <div class="publication"><span>FLiNG Trainer</span>{#if entry.updatedAt || entry.publishedAt}<span>{entry.updatedAt ? '更新于' : '发布于'} <time datetime={entry.updatedAt ?? entry.publishedAt ?? undefined}>{dateLabel(entry.updatedAt ?? entry.publishedAt)}</time></span>{/if}</div>
      </header>
      {#if detailBusy && !detail}<div class="plain-state" role="status"><Icon name="loader" class="spinning" size={17} />正在读取官网详情…</div>{/if}
      {#if detailError}<div class="state-error" role="alert"><Icon name="alert" size={18} /><div><strong>{detail ? '刷新失败，仍显示上次的详情' : '详情未能载入'}</strong><p>{detailError}</p></div><button class="button secondary" onclick={() => entry && void openEntry(entry, true)}>重试</button></div>{/if}
      {#if detail}
        <section class="attachments" aria-labelledby="files-heading"><div class="detail-section-heading"><h3 id="files-heading">官方附件</h3><span>{files.length} 个</span></div>
          <p class="save-help">{preview ? '示例附件仅供预览，桌面版可保存官网文件。' : externalDownloaderName ? `可发送到 ${externalDownloaderName}，也可另存到本机。追新不会运行附件。` : '选择附件另存到本机；追新不会运行下载的文件。'}</p>
          {#each (showAllFiles ? files : files.slice(0, 4)) as file (file.url)}
            {@const prior = externalDownloaderName ? handoffs.get(receiptKey(externalDownloaderName, file)) : undefined}
            <div class="attachment-row"><Icon name="notes" size={18} /><span>{file.label}</span>
              <div class="attachment-actions">
                {#if externalDownloaderName}
                  <button class="button secondary handoff-key" disabled={preview || actionBusy || !!prior} aria-label={`将 ${file.label} 发送到 ${externalDownloaderName}`} title={prior ? handoffMessage(externalDownloaderName, prior) : `发送到 ${externalDownloaderName}`} onclick={() => void send(file)}><Icon name={sending === file.url ? 'loader' : prior === 'unknown' ? 'info' : prior ? 'check' : 'external'} class={sending === file.url ? 'spinning' : ''} size={15} /><span>{sending === file.url ? '正在发送' : prior === 'unknown' ? '请先检查队列' : prior ? '已发送' : '发送到下载器'}</span></button>
                {/if}
                <button class:button={!externalDownloaderName} class:secondary={!externalDownloaderName} class:text-button={!!externalDownloaderName} disabled={preview || actionBusy} aria-label={`另存为 ${file.label}`} onclick={() => void save(file)}><Icon name={saving === file.url ? 'loader' : 'download'} class={saving === file.url ? 'spinning' : ''} size={15} />{saving === file.url ? '正在保存' : '另存为…'}</button>
              </div>
            </div>
          {:else}<p class="no-files">此条目没有可在追新保存的官方附件，可查看官网原文。</p>{/each}
          {#if files.length > 4}<button class="text-button files-more" aria-expanded={showAllFiles} onclick={() => showAllFiles = !showAllFiles}>{showAllFiles ? '收起附件' : `查看全部 ${files.length} 个附件`}<Icon name="chevronDown" size={14} /></button>{/if}
          {#if saveNote}<div class="save-result"><p class="save-feedback" role="status"><Icon name={savedPath ? 'check' : 'info'} size={15} /><span>{saveNote}{savedPath ? ` ${savedPath}` : ''}</span></p>{#if savedPath}<button class="text-button" onclick={() => void reveal()}><Icon name="folder" size={14} />在文件夹中显示</button>{/if}</div>{/if}
        </section>
        {#if detail.content || detail.summary}<section class="read-section"><h3>官网说明</h3><div class="official-text">{detail.content || detail.summary}</div></section>{/if}
      {/if}
    </article>
  {:else}
    <section aria-labelledby="directory-heading" aria-busy={listBusy}>
      <div class="directory-heading"><div><h2 id="directory-heading">{mode === 'search' ? '搜索结果' : '修改器目录'}</h2>{#if mode === 'search'}<span class="term">“{query}”</span>{/if}</div>
        <div class="directory-tools">{#if listing}<span>本页 <b>{listing.items.length}</b> 项</span>{/if}<button class="icon-button" aria-label="刷新目录" title="刷新目录" disabled={listBusy} onclick={() => void loadList(mode, query, page, true)}><Icon name={listBusy ? 'loader' : 'refresh'} class={listBusy ? 'spinning' : ''} size={15} /></button></div>
      </div>
      {#if listError}<div class="state-error" role="alert"><Icon name="alert" size={18} /><div><strong>{listing ? '刷新失败，仍显示上次的目录' : '官网目录未能载入'}</strong><p>{listError}</p></div><button class="button secondary" onclick={() => void loadList(mode, query, page, true)}>重试</button></div>{/if}
      {#if listing?.items.length}
        <div class="game-columns" aria-hidden="true"><span>游戏名称</span><span>官网日期</span></div>
        <ul class="game-list" aria-label={listName}>
          {#each listing.items as item (item.url)}<li>
            <button class="game-row" data-url={item.url} aria-label={`查看 ${item.title}`} onclick={() => void openEntry(item)}>
              <span class="game-copy"><strong>{gameName(item.title)}</strong>{#if item.summary}<span>{item.summary}</span>{/if}</span>
              <time datetime={item.updatedAt ?? item.publishedAt ?? undefined}>{dateLabel(item.updatedAt ?? item.publishedAt)}</time><Icon name="chevronRight" size={17} />
            </button>
          </li>{/each}
        </ul>
        <footer class="directory-footer"><span>{preview ? '开发预览 · 示例目录' : '来源：flingtrainer.com'}</span>
          <div class="page-controls"><button class="icon-button" aria-label="上一页" disabled={listBusy || page <= 1} onclick={() => void loadList(mode, query, page - 1)}><Icon name="chevronRight" class="reverse" size={16} /></button>
            <span>第 {page} 页{listing.totalPages ? ` / 共 ${listing.totalPages} 页` : ''}</span><button class="icon-button" aria-label="下一页" disabled={listBusy || !listing.hasNextPage} onclick={() => void loadList(mode, query, page + 1)}><Icon name="chevronRight" size={16} /></button></div>
        </footer>
      {:else if listBusy}<div class="directory-state" role="status"><Icon name="loader" class="spinning" size={23} /><h3>正在读取官网目录</h3><p>flingtrainer.com</p></div>
      {:else if !listError}<div class="directory-state" role="status"><Icon name={mode === 'search' ? 'search' : 'readme'} size={24} /><h3>{mode === 'search' ? '没有找到匹配的游戏' : '暂无条目'}</h3><p>{mode === 'search' ? '试试游戏英文名，或只输入名称中的几个词。' : '官网暂未返回目录，可稍后刷新。'}</p>{#if mode === 'search'}<button class="button secondary" onclick={() => browse(browseMode)}>返回目录</button>{/if}</div>{/if}
    </section>
  {/if}
  {#if actionError}<p class="save-feedback action-error" role="alert"><Icon name="alert" size={16} />{actionError}</p>{/if}
</section>

<style>
  .trainer-page { --row-px:14px; --date-col:90px; display:block; min-width:0; color:var(--text); }
  .page-heading { margin-bottom:21px; }
  .original { flex:none; }
  .trainer-toolbar, .trainer-toolbar form, .directory-tools, .page-controls, .directory-heading > div, .publication, .back-link, .detail-section-heading, .plain-state, .save-feedback, .files-more { display:flex; align-items:center; }
  .trainer-toolbar { flex-wrap:wrap; justify-content:space-between; gap:14px 24px; padding-bottom:26px; }
  .trainer-toolbar form { gap:10px; flex:1 1 210px; max-width:358px; min-width:0; }
  .trainer-toolbar .trainer-search { flex:1; min-width:0; padding-inline:12px; }
  .trainer-search input { min-width:0; font-size:13px; }
  .trainer-search input::placeholder { color:var(--muted); opacity:1; }
  .trainer-tabs { flex:none; }
  .trainer-page h2 { font-size:18px; font-weight:660; line-height:1.35; }
  .trainer-page h3 { font-size:15px; line-height:1.5; font-weight:650; }
  .directory-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; margin:2px 0 18px; }
  .directory-heading > div { gap:12px; min-width:0; }
  .directory-heading h2 { flex:none; }
  .term { font-size:14px; color:var(--subtle); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .directory-tools { gap:12px; flex:none; color:var(--muted); font-size:12px; }
  .directory-tools b { font-weight:600; color:var(--text); }
  .directory-tools .icon-button, .page-controls .icon-button { width:30px; height:30px; }
  .game-columns, .game-row { display:grid; grid-template-columns:minmax(0,1fr) var(--date-col) 17px; align-items:center; column-gap:18px; padding-inline:var(--row-px); }
  .game-columns { padding-block:0 9px; border-bottom:1px solid var(--hairline); color:var(--muted); font-size:12px; }
  .game-columns span:nth-child(2), .game-row time { justify-self:end; }
  .game-list { margin:0; padding:0; list-style:none; }
  .game-list li { position:relative; }
  .game-list li:not(:last-child)::after { content:''; position:absolute; inset:auto var(--row-px) 0; height:1px; background:var(--hairline); pointer-events:none; }
  .game-row { position:relative; width:100%; min-height:54px; padding-block:12px; text-align:start; color:var(--text); background:transparent; border-radius:var(--r-key); transition:background-color 140ms ease,box-shadow 180ms ease; }
  .game-row:hover { background:var(--quiet-hover); }
  .game-row:active { color:var(--cobalt); box-shadow:var(--inset-sm); }
  .game-row:focus-visible { outline-offset:-2px; }
  .game-copy { min-width:0; display:flex; flex-direction:column; gap:3px; }
  .game-copy strong { font-size:15px; line-height:1.4; font-weight:620; overflow-wrap:anywhere; }
  .game-copy > span { display:block; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:12px; line-height:1.5; color:var(--muted); }
  .game-row time { font-size:12px; color:var(--muted); font-variant-numeric:tabular-nums; }
  .game-row :global(svg) { color:var(--muted); transition:transform 180ms var(--spring); }
  .game-row:hover :global(svg) { transform:translateX(2px); }
  .directory-footer { display:flex; align-items:center; justify-content:space-between; flex-wrap:wrap; gap:12px; padding:16px var(--row-px) 4px; border-top:1px solid var(--hairline); color:var(--muted); font-size:12px; }
  .page-controls { gap:12px; }
  :global(.trainer-page .reverse) { transform:rotate(180deg); }
  .directory-state { display:grid; justify-items:center; align-content:center; gap:7px; padding:52px 16px; text-align:center; color:var(--muted); }
  .directory-state h3 { margin-top:4px; } .directory-state p, .state-error p { font-size:13px; line-height:1.55; }
  .directory-state button { margin-top:10px; }
  .state-error { display:flex; align-items:center; flex-wrap:wrap; gap:12px; padding:14px 0; margin:0 0 12px; }
  .state-error :global(svg), .action-error { color:var(--danger); }
  .state-error > div { flex:1 1 210px; } .state-error strong { font-size:14px; } .state-error p { color:var(--muted); overflow-wrap:anywhere; }
  .back-link { gap:6px; color:var(--subtle); font-size:13px; border-radius:var(--r-key); padding:5px 0; font-weight:600; }
  .back-link:hover { color:var(--heading); text-decoration:underline; text-underline-offset:3px; }
  .detail-nav { display:flex; align-items:center; justify-content:space-between; gap:16px; border-top:1px solid var(--hairline); padding-top:15px; }
  .detail-nav .icon-button { width:30px; height:30px; }
  .game-heading { margin:16px 0 25px; } .game-heading h2 { font-size:clamp(21px,2vw,28px); overflow-wrap:anywhere; }
  .game-heading h2:focus:not(:focus-visible) { outline:none; }
  .publication { flex-wrap:wrap; gap:8px 14px; margin-top:7px; color:var(--muted); font-size:12px; }
  .publication span + span { padding-inline-start:14px; border-inline-start:1px solid var(--border-strong); }
  .plain-state { gap:8px; color:var(--muted); padding-block:20px; }
  .detail-section-heading { gap:9px; } .detail-section-heading span, .save-help { color:var(--muted); font-size:12px; }
  .save-help { margin:3px 0 12px; }
  .attachment-row { display:flex; align-items:center; gap:12px; padding:12px 0; border-top:1px solid var(--hairline); }
  .attachment-row :global(svg) { flex:none; color:var(--subtle); } .attachment-row > span { flex:1; min-width:0; overflow-wrap:anywhere; font-size:13px; }
  .attachment-actions { display:flex; align-items:center; justify-content:flex-end; gap:7px 12px; flex-wrap:wrap; max-width:55%; }
  .attachment-actions button { flex:none; } .attachment-actions .text-button { display:inline-flex; align-items:center; gap:5px; }
  .files-more { margin:10px 0; gap:5px; }
  .save-feedback { gap:7px; font-size:13px; line-height:1.6; margin:14px 0; overflow-wrap:anywhere; } .save-feedback :global(svg) { flex:none; }
  .save-result { display:flex; align-items:center; gap:6px 16px; flex-wrap:wrap; } .save-result .text-button { flex:none; } .save-result .save-feedback { min-width:0; }
  .save-feedback:not(.action-error) { color:var(--subtle); } .no-files { padding:10px 0; color:var(--muted); font-size:13px; }
  .read-section { margin-top:24px; border-top:1px solid var(--hairline); padding:20px 0 12px; }
  .official-text { white-space:pre-line; overflow-wrap:anywhere; color:var(--subtle); font-size:14px; line-height:1.75; max-width:74ch; margin-top:7px; }
  @media (max-width:950px) { .trainer-page { --row-px:8px; --date-col:78px; } .trainer-toolbar { gap:12px; } .game-row, .game-columns { column-gap:10px; } }
  @media (max-width:650px) { .trainer-toolbar form { max-width:none; flex-basis:100%; } .game-row, .game-columns { column-gap:6px; } .page-heading { align-items:flex-start; } }
  @media (forced-colors:active) { .game-row:active, .game-row:hover { outline:1px solid Highlight; } }
</style>
