<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import type { GithubFeedPage, GithubRepository } from '../lib/types';
  import { formatDate, readableError, safeGithubUrl, type Tone } from '../lib/ui';
  import type { FeedPreviewState } from '../lib/feed/preview';
  import Icon from './Icon.svelte';
  import GithubAvatar from './GithubAvatar.svelte';

  let { preview, onopen, onadd, onnotice }: {
    preview: boolean;
    onopen: (url: string) => void;
    onadd: (repository: GithubRepository) => Promise<boolean>;
    onnotice: (message: string, tone?: Tone) => void;
  } = $props();
  type FeedAction = 'feed_connect' | 'feed_refresh' | 'feed_more' | 'feed_disconnect';
  let page = $state<GithubFeedPage | null>(null);
  let pending = $state<FeedAction | null>(null);
  let initialLoading = $state(true);
  let transportError = $state('');
  let previewState = $state<FeedPreviewState>('connected');
  let disconnectArmed = $state(false);
  let adding = $state(new Set<string>());
  let added = $state(new Set<string>());
  let expanded = $state(new Set<string>());
  let destroyed = false;
  let request = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const connected = $derived(Boolean(page?.connected && page.account));
  const error = $derived(transportError || page?.error || '');
  const busy = $derived(Boolean(pending) || Boolean(page?.loading));
  const sourceLabel = $derived(page?.sourceUrl && safeGithubUrl(page.sourceUrl) && new URL(page.sourceUrl).pathname === '/' ? 'GitHub 官网首页' : '官网 Dashboard feed');

  function stopPoll() { if (timer !== undefined) clearTimeout(timer); timer = undefined; }
  function schedulePoll() {
    stopPoll();
    if (destroyed || preview) return;
    timer = setTimeout(() => { timer = undefined; void refreshStatus(); }, page?.loading || pending === 'feed_connect' ? 1200 : 5000);
  }
  function accept(next: GithubFeedPage) {
    if (page && next.sessionRevision < page.sessionRevision) return;
    if (page?.account !== next.account || page?.sessionRevision !== next.sessionRevision) {
      adding = new Set(); added = new Set(); expanded = new Set(); disconnectArmed = false;
    }
    page = next;
  }
  async function refreshStatus() {
    if (destroyed || pending) { schedulePoll(); return; }
    const current = ++request;
    try {
      const next = preview && import.meta.env.DEV
        ? (await import('../lib/feed/preview')).previewFeed(previewState)
        : await dispatch('feed_status', {});
      if (!destroyed && current === request) { accept(next); transportError = ''; }
    } catch (failure) { if (!destroyed && current === request) transportError = readableError(failure); }
    finally { if (!destroyed && current === request) { initialLoading = false; schedulePoll(); } }
  }
  async function action(name: FeedAction) {
    if (preview || destroyed || pending || (page?.loading && name === 'feed_more')) return;
    stopPoll();
    const current = ++request;
    pending = name; transportError = ''; disconnectArmed = false;
    if (name === 'feed_disconnect') {
      // Hide old account data before the round-trip, including an in-flight add.
      adding = new Set(); added = new Set(); expanded = new Set();
      if (page) page = { ...page, account: null, connected: false, loading: false, items: [], hasMore: false, lastUpdated: null };
    }
    try {
      const next = await dispatch(name, {});
      if (!destroyed && current === request) {
        accept(next);
        if (name === 'feed_disconnect' && !next.error) onnotice('已退出本机 GitHub 网页会话。', 'success');
      }
    } catch (failure) { if (!destroyed && current === request) transportError = readableError(failure); }
    finally { if (!destroyed && current === request) { pending = null; initialLoading = false; schedulePoll(); } }
  }
  function openLink(value: string) {
    const safe = safeGithubUrl(value);
    if (safe) onopen(safe);
  }
  async function addRepository(repository: string) {
    if (preview || destroyed || adding.has(repository) || added.has(repository)) return;
    const account = page?.account;
    const revision = page?.sessionRevision;
    const current = () => !destroyed && page?.account === account && page?.sessionRevision === revision;
    adding = new Set([...adding, repository]);
    try {
      const detail = await dispatch('github_repository', { repository });
      if (!current()) return;
      if (detail.repository.private) { onnotice('私有仓库暂不加入软件库。', 'warning'); return; }
      const saved = await onadd(detail.repository);
      if (current() && saved) added = new Set([...added, repository]);
    } catch (failure) { if (current()) onnotice(readableError(failure), 'danger'); }
    finally { if (current()) { const next = new Set(adding); next.delete(repository); adding = next; } }
  }
  function toggleBody(id: string) { const next = new Set(expanded); if (next.has(id)) next.delete(id); else next.add(id); expanded = next; }
  function changePreview() { ++request; void refreshStatus(); }
  onMount(() => {
    void refreshStatus();
    return () => { destroyed = true; ++request; stopPoll(); };
  });
</script>

<section class="feed" aria-label="GitHub 官网首页动态">
  <header class="feed-toolbar">
    <div class="feed-title">
      <h2>首页动态</h2>
      <p>{#if initialLoading}正在读取连接状态…{:else if connected}@{page?.account} · 网站已登录{:else}连接 GitHub 官网，读取你的动态{/if}</p>
    </div>
    <div class="feed-actions">
      {#if connected}
        <button class="button secondary small" disabled={preview || Boolean(pending)} onclick={() => void action('feed_connect')}><Icon name="eye" size={14} />官网窗口</button>
        <button class="button secondary small" disabled={preview || busy} onclick={() => void action('feed_refresh')}><Icon name="refresh" size={14} />刷新</button>
        <button class="text-button feed-disconnect" disabled={preview || Boolean(pending)} onclick={() => disconnectArmed = !disconnectArmed}>退出网页</button>
      {:else}
        <button class="button primary" disabled={preview || Boolean(pending) || initialLoading} onclick={() => void action('feed_connect')}><Icon name={pending === 'feed_connect' ? 'loader' : 'github'} class={pending === 'feed_connect' ? 'spinning' : ''} size={16} />{pending === 'feed_connect' ? '正在打开' : page?.loading ? '继续网页登录' : '连接官网'}</button>
      {/if}
    </div>
  </header>

  {#if preview}
    <div class="feed-preview"><p><strong>首页动态示例</strong> · 以下内容为演示，不是真实账号动态。</p><label>示例状态<select aria-label="首页动态示例状态" bind:value={previewState} onchange={changePreview}><option value="connected">已连接</option><option value="login">未登录</option><option value="loading">读取中</option><option value="empty">空动态</option><option value="error">读取失败</option></select></label></div>
  {/if}
  {#if disconnectArmed}
    <div class="feed-confirm" role="group" aria-label="确认退出网页登录"><p>清除追新保存的网页登录。浏览器和 GitHub 授权连接不受影响。</p><div><button class="button secondary small" onclick={() => disconnectArmed = false}>取消</button><button class="button secondary small" onclick={() => void action('feed_disconnect')}>确认退出</button></div></div>
  {/if}
  {#if error}
    <div class="feed-error" role="alert"><Icon name="alert" size={17} /><p>{error}</p><button class="button secondary small" disabled={preview || Boolean(pending)} onclick={() => void action(connected ? 'feed_refresh' : 'feed_connect')}>重试</button></div>
  {/if}

  <div class="feed-content" aria-busy={initialLoading || busy}>
    {#if !connected && !initialLoading}
      <div class="feed-empty">
        <Icon name="activity" size={27} />
        <h3>{page?.loading ? '在官网窗口完成登录' : '把官网动态带回这里'}</h3>
        <p>首次使用需要再登录一次 GitHub 网站。完成后，动态会自动显示在这里，官网窗口随即隐藏。</p>
        <p class="feed-local-note">此网页登录会保存在这台电脑，下次打开可继续使用。</p>
      </div>
    {:else if initialLoading || (page?.loading && !page.items.length)}
      <div class="feed-loading" role="status"><Icon name="loader" class="spinning" size={20} /><div><h3>正在读取官网动态</h3><p>等待 GitHub 页面加载，保留原页面顺序。</p></div></div>
    {:else if page?.items.length}
      <ol class="feed-list">
        {#each page.items as item (item.id)}
          <li><article class="feed-row">
            <GithubAvatar url={item.avatarUrl} name={item.actor} />
            <div class="feed-copy">
              <div class="feed-byline"><strong>{item.actor}</strong>{#if item.occurredAt}<time datetime={item.occurredAt}>{formatDate(item.occurredAt, true)}</time>{/if}</div>
              <h3><button class="feed-summary" onclick={() => openLink(item.url)}>{item.summary}</button></h3>
              {#if item.body}<p class="feed-body" class:collapsed={item.body.length > 220 && !expanded.has(item.id)} id={`feed-body-${item.id}`}>{item.body}</p>{#if item.body.length > 220}<button class="text-button" aria-expanded={expanded.has(item.id)} aria-controls={`feed-body-${item.id}`} onclick={() => toggleBody(item.id)}>{expanded.has(item.id) ? '收起内容' : '展开内容'}</button>{/if}{/if}
              <div class="feed-row-actions">
                {#if item.repository}<button class="text-button feed-repository" onclick={() => openLink(`https://github.com/${item.repository}`)}><Icon name="readme" size={14} />{item.repository}</button>{/if}
                <button class="text-button" onclick={() => openLink(item.url)}>查看原文<Icon name="external" size={13} /></button>
                {#if item.repository}<button class="button secondary small" disabled={preview || adding.has(item.repository) || added.has(item.repository)} onclick={() => { if (item.repository) void addRepository(item.repository); }}><Icon name={adding.has(item.repository) ? 'loader' : added.has(item.repository) ? 'check' : 'plus'} class={adding.has(item.repository) ? 'spinning' : ''} size={13} />{added.has(item.repository) ? '已加入' : adding.has(item.repository) ? '正在加入' : '加入软件库'}</button>{/if}
              </div>
            </div>
          </article></li>
        {/each}
      </ol>
    {:else if !error}
      <div class="feed-empty"><Icon name="activity" size={27} /><h3>官网暂时没有新动态</h3><p>这里显示 GitHub 实际加载的内容，稍后可以再刷新。</p><button class="button secondary small" disabled={preview || busy} onclick={() => void action('feed_refresh')}>刷新动态</button></div>
    {/if}
    {#if connected && page && (page.items.length || page.lastUpdated)}
      <footer class="feed-footer"><span>{page.loading ? '正在从官网读取…' : `已读取 ${page.items.length} 条`}{#if page.lastUpdated} · 更新于 {formatDate(page.lastUpdated, true)}{/if}</span>{#if page.hasMore}<button class="button secondary small" disabled={preview || busy} onclick={() => void action('feed_more')}><Icon name={busy ? 'loader' : 'chevronDown'} class={busy ? 'spinning' : ''} size={14} />{pending === 'feed_more' || page.loading ? '正在加载' : '加载更多'}</button>{:else}<span>{page.items.length >= 160 ? '已达到本次读取上限，可刷新重新读取' : '已到官网当前内容末尾'}</span>{/if}</footer>
    {/if}
  </div>
  <p class="feed-source"><Icon name="github" size={14} /><span>来源：{sourceLabel} · 按网页顺序展示</span><button class="text-button" onclick={() => openLink(page?.sourceUrl || 'https://github.com/dashboard-feed')}>打开来源<Icon name="external" size={12} /></button></p>
</section>

<style>
  .feed { min-width: 0; }
  .feed-toolbar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; margin-bottom: 14px; }
  .feed-title h2 { font-size: 16px; }
  .feed-title p { margin-top: 3px; color: var(--muted); font-size: 12px; }
  .feed-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .feed-disconnect { margin-left: 4px; color: var(--muted); }
  .feed-content { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
  .feed-list { margin: 0; padding: 0; list-style: none; }
  .feed-list > li + li { border-top: 1px solid var(--border); }
  .feed-row { display: flex; align-items: flex-start; gap: 13px; padding: 19px 20px; }
  .feed-copy { flex: 1; min-width: 0; }
  .feed-byline { display: flex; align-items: baseline; flex-wrap: wrap; gap: 8px 14px; margin-bottom: 5px; color: var(--subtle); font-size: 12px; }
  .feed-byline strong { overflow-wrap: anywhere; font-weight: 600; }
  .feed-byline time { font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .feed-copy h3 { font-size: 14px; line-height: 1.55; }
  .feed-summary { display: block; color: var(--heading); text-align: left; font-weight: 600; padding: 0; overflow-wrap: anywhere; line-height: inherit; }
  .feed-summary:hover { color: var(--accent); text-decoration: underline; text-underline-offset: 3px; }
  .feed-body { color: var(--subtle); font-size: 12.5px; line-height: 1.75; margin-top: 7px; max-width: 75ch; overflow-wrap: anywhere; white-space: pre-wrap; }
  .feed-body.collapsed { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; overflow: hidden; }
  .feed-row-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; margin-top: 13px; }
  .feed-repository { min-width: 0; overflow-wrap: anywhere; }
  .feed-repository + .text-button { margin-left: auto; }
  .feed-empty { padding: 34px 26px; display: flex; flex-direction: column; align-items: center; text-align: center; gap: 11px; color: var(--muted); }
  .feed-empty h3 { font-size: 15px; }
  .feed-empty p { max-width: 58ch; font-size: 12px; }
  .feed-empty .feed-local-note { font-size: 11px; }
  .feed-loading { min-height: 170px; display: flex; justify-content: center; align-items: center; gap: 14px; padding: 25px; color: var(--muted); }
  .feed-loading h3 { font-size: 14px; margin-bottom: 5px; }
  .feed-loading p { font-size: 12px; }
  .feed-footer { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 10px; padding: 12px 20px; border-top: 1px solid var(--border); background: var(--surface-soft); color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .feed-source { display: flex; align-items: center; gap: 7px 10px; flex-wrap: wrap; color: var(--muted); font-size: 11px; margin-top: 10px; }
  .feed-source .text-button { margin-left: auto; font-size: 11px; }
  .feed-preview, .feed-confirm { margin-bottom: 12px; display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 10px; padding: 10px 13px; font-size: 11px; border: 1px solid var(--border); border-radius: 6px; }
  .feed-preview { background: var(--warning-bg); color: var(--warning); }
  .feed-preview label { display: flex; align-items: center; gap: 8px; }
  .feed-preview select { padding: 4px 7px; font-size: 11px; }
  .feed-confirm { color: var(--subtle); background: var(--surface-soft); }
  .feed-confirm > div { display: flex; gap: 8px; }
  .feed-error { display: flex; align-items: flex-start; gap: 9px; padding: 12px 14px; margin-bottom: 12px; background: var(--danger-bg); color: var(--danger); border-radius: 6px; font-size: 12px; }
  .feed-error > :global(svg) { margin-top: 2px; }
  .feed-error p { flex: 1; overflow-wrap: anywhere; }
  @media (max-width: 1130px) { .feed-row { padding: 17px 15px; gap: 10px; } .feed-row-actions { gap: 8px 12px; } .feed-repository { width: 100%; } .feed-repository + .text-button { margin-left: 0; } }
  @media (prefers-reduced-motion: reduce) { :global(.feed .spinning) { animation: none; } }
</style>
