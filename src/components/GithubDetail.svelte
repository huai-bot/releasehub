<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';
  import { dispatch, type Actions } from '../lib/bridge';
  import { handoffMessage } from '../lib/external-handoff';
  import type { GithubRepository, GithubRepositoryDetail, RepositoryNote } from '../lib/types';
  import { formatBytes, formatDate, readableError, safeGithubUrl } from '../lib/ui';
  import Icon from './Icon.svelte';
  import ReleaseNotes from './ReleaseNotes.svelte';
  let { name, detail, note, loading, error, connected, preview, pending, subscribed, adding, externalDownloaderName = null, backLabel = '返回列表', onclose, onretry, onstar, onedit, onnote, onlists, onadd, onopen, onoriginal }: {
    backLabel?: string; onoriginal?: (url: string) => void; externalDownloaderName?: string | null;
    name: string; detail: GithubRepositoryDetail | null; note?: RepositoryNote; loading: boolean; error: string;
    connected: boolean; preview: boolean; pending: boolean; subscribed: boolean; adding: boolean;
    onclose: () => void; onretry: () => void; onstar: (repository: GithubRepository) => void;
    onedit: (repository: GithubRepository) => void; onnote: (repository: string) => void; onlists: (repository: GithubRepository) => void;
    onadd: (repository: GithubRepository) => void; onopen: (url: string) => void;
  } = $props();
  const repository = $derived(detail?.repository ?? null);
  type ReleaseListing = NonNullable<Actions['github_release_assets']['result']>;
  let listingFor = $state('');
  let listing = $state<ReleaseListing | null>(null);
  let listingBusy = $state(false);
  let listingError = $state('');
  type DownloadMethod = 'internal' | 'external';
  let downloading = $state<{ url: string; method: DownloadMethod } | null>(null);
  let downloadFeedback = $state<{ url: string; text: string } | null>(null);
  const externalReceipts = new SvelteMap<string, string>();
  let listingRequest = 0;
  const showListing = $derived(listingFor === name);

  async function readAssets() {
    if (!repository || repository.private || preview || downloading || (showListing && listingBusy)) return;
    const requested = repository.fullName;
    const request = ++listingRequest;
    listingFor = name; listing = null; listingBusy = true; listingError = ''; downloadFeedback = null;
    try {
      const result = await dispatch('github_release_assets', { repository: requested });
      if (request === listingRequest) listing = result;
    } catch (reason) { if (request === listingRequest) listingError = readableError(reason); }
    finally { if (request === listingRequest) listingBusy = false; }
  }

  async function downloadAsset(asset: ReleaseListing['assets'][number], method: DownloadMethod) {
    const downloader = externalDownloaderName;
    const { url } = asset;
    if (!showListing || downloading || (method === 'external' && (!downloader || externalReceipts.has(url)))) return;
    downloading = { url, method };
    downloadFeedback = null;
    try {
      if (method === 'external' && downloader) {
        const receipt = await dispatch('send_external_release_link', { url, confirmed: true });
        externalReceipts.set(url, handoffMessage(downloader, receipt.status));
      } else {
        const record = await dispatch('download_release_link', { url });
        downloadFeedback = { url, text: record.status === 'completed' ? `${record.fileName} 已在下载中心。` : `${record.fileName} 已加入下载队列。` };
      }
    } catch (reason) { downloadFeedback = { url, text: readableError(reason) }; }
    finally { downloading = null; }
  }
</script>

<aside class="github-detail" aria-label={`${name} 仓库详情`} aria-busy={loading}>
  <div class="repo-detail-top"><button class="text-button" onclick={onclose}><Icon name="chevronRight" class="back-arrow" size={15} />{backLabel}</button><button class="icon-button" aria-label="刷新仓库详情" disabled={loading || pending} onclick={onretry}><Icon name="refresh" size={16} /></button></div>
  {#if loading}
    <div class="detail-body" role="status"><p class="muted">正在读取 {name}…</p><div class="skeleton-lines"><span></span><span></span><span></span></div></div>
  {:else if error}
    <div class="empty-state"><Icon name="alert" size={26} /><h3>仓库详情暂时没有读到</h3><p class="break-word" role="alert">{error}</p><button class="button secondary" onclick={onretry}>重新读取</button>{#if onoriginal}<button class="text-button" onclick={() => onoriginal?.(`https://github.com/${name}`)}>查看原页</button>{/if}</div>
  {:else if repository}
    <div class="detail-body">
      <div class="repo-heading"><Icon name="readme" size={21} /><h2>{note?.name || repository.fullName}</h2></div>
      {#if note?.description}<p class="local-note">{note.description}</p>{/if}
      {#if note?.name}<p class="original-name">{repository.fullName}</p>{/if}
      {#if repository.description}<p class="repo-description">{repository.description}</p>{/if}
      <div class="repo-flags">{#if repository.private}<span class="status-badge neutral"><Icon name="private" size={12} />私有</span>{:else}<span class="status-badge neutral">公开</span>{/if}{#if repository.archived}<span class="status-badge warning">已归档</span>{/if}{#if repository.fork}<span class="status-badge neutral">Fork</span>{/if}{#if preview}<span class="status-badge warning">示例仓库</span>{/if}</div>
      {#if repository.homepage}<div class="repo-homepage">{#if safeGithubUrl(repository.homepage)}<button class="text-button" onclick={() => onopen(repository.homepage!)}>{repository.homepage}<Icon name="external" size={13} /></button>{:else}<span>{repository.homepage}</span>{/if}</div>{/if}
      <div class="repo-statline"><span><Icon name="star" size={14} />{repository.stars.toLocaleString()} Stars</span><span><Icon name="fork" size={14} />{repository.forks.toLocaleString()} Forks</span>{#if repository.language}<span>{repository.language}</span>{/if}<span>{formatDate(repository.updatedAt)}</span></div>
      {#if repository.topics.length}<div class="repo-topics">{#each repository.topics as topic}<span>{topic}</span>{/each}</div>{/if}
      <div class="repo-actions">
        <button class="button secondary" disabled={!connected || repository.starred === null || pending || preview} onclick={() => onstar(repository)}><Icon name={pending ? 'loader' : 'star'} class={pending ? 'spinning' : ''} size={15} />{pending ? '正在保存' : !connected ? '登录后可 Star' : repository.starred === null ? 'Star 状态未知' : repository.starred ? '取消 Star' : 'Star'}</button>
        <button class="button primary" disabled={preview || repository.private || subscribed || adding} onclick={() => onadd(repository)}><Icon name={adding ? 'loader' : subscribed ? 'check' : 'plus'} class={adding ? 'spinning' : ''} size={15} />{subscribed ? '已订阅' : adding ? '正在加入' : '加入软件库'}</button>
        <button class="button secondary" disabled={preview} onclick={() => onnote(repository.fullName)}><Icon name="notes" size={15} />{note ? '编辑本地备注' : '添加本地备注'}</button>
        {#if connected && repository.starred}<button class="button secondary" disabled={pending || preview} onclick={() => onlists(repository)}><Icon name="folder" size={15} />设置分类</button>{/if}
        {#if repository.canAdmin && connected}<button class="button secondary" disabled={pending} onclick={() => onedit(repository)}><Icon name="edit" size={15} />修改说明与主页</button>{/if}
      </div>
      {#if repository.private || (connected && repository.starred === null)}<p class="field-hint">{repository.private ? '私有仓库暂不加入软件库。' : ''}{#if connected && repository.starred === null} Star 状态未读到，请刷新详情后再操作。{/if}</p>{/if}
      <div class="repo-pages">
        {#if !repository.private && !preview}<button class="text-button" disabled={Boolean(downloading) || (showListing && listingBusy)} onclick={() => void readAssets()}><Icon name={showListing && listingBusy ? 'loader' : 'package'} class={showListing && listingBusy ? 'spinning' : ''} size={15} />{showListing ? '刷新最新附件' : '查看最新附件'}</button>{/if}
        <button class="text-button" onclick={() => onopen(`${repository.url}/releases`)}>发行版原文</button>
        {#if repository.hasIssues}<button class="text-button" onclick={() => onopen(`${repository.url}/issues`)}>问题</button>{/if}
        {#if repository.hasDiscussions}<button class="text-button" onclick={() => onopen(`${repository.url}/discussions`)}>讨论</button>{/if}
        <button class="text-button" onclick={() => (onoriginal ?? onopen)(repository.url)}>查看原页<Icon name="chevronRight" size={14} /></button>
      </div>
      {#if showListing && !repository.private}
        <section class="release-assets" aria-label="最新发行附件" aria-busy={listingBusy}>
          {#if listingBusy}<p role="status">正在读取最新公开发行版…</p>
          {:else if listingError}<p class="field-error" role="alert">无法读取附件：{listingError}。可刷新重试，或打开发行版原文。</p>
          {:else if listing}
            <div class="release-assets-heading"><h3>最新附件 · {listing.tag}</h3><button class="text-button" onclick={() => listing && onopen(listing.url)}>查看此发行版</button></div>
            {#if listing.assets.length}
              <ul>
                {#each listing.assets as asset (asset.id)}
                  {@const sending = downloading?.url === asset.url && downloading.method === 'external'}
                  {@const queuing = downloading?.url === asset.url && downloading.method === 'internal'}
                  {@const receipt = externalReceipts.get(asset.url)}
                  {@const internalLabel = externalDownloaderName ? '内置下载' : '下载'}
                  <li>
                    <div class="asset-file"><strong>{asset.name}</strong><span>{formatBytes(asset.size)}</span></div>
                    <div class="asset-actions">
                      {#if externalDownloaderName}<button class="button secondary" aria-label={`发送到 ${externalDownloaderName}：${asset.name}`} disabled={Boolean(downloading || receipt)} onclick={() => void downloadAsset(asset, 'external')}><Icon name={sending ? 'loader' : 'external'} class={sending ? 'spinning' : ''} size={15} />{sending ? '正在发送' : `发送到 ${externalDownloaderName}`}</button>{/if}
                      <button class="button secondary" aria-label={`${internalLabel} ${asset.name}`} disabled={Boolean(downloading)} onclick={() => void downloadAsset(asset, 'internal')}><Icon name={queuing ? 'loader' : 'download'} class={queuing ? 'spinning' : ''} size={15} />{queuing ? '正在核对' : internalLabel}</button>
                    </div>
                    {#if receipt}<p class="asset-feedback" role="status">{receipt}</p>{/if}
                    {#if downloadFeedback?.url === asset.url}<p class="asset-feedback" role="status">{downloadFeedback.text}</p>{/if}
                  </li>
                {/each}
              </ul>
            {:else}<p role="status">此发行版没有可下载的附件。</p>{/if}
          {:else}<p role="status">未找到最新公开发行版。可打开原文查看其他版本。</p>{/if}
        </section>
      {/if}
      <div class="readme-heading"><Icon name="notes" size={16} /><h3>README</h3></div>
      {#if detail?.readme.trim()}<ReleaseNotes body={detail.readme} repositoryUrl={`${repository.url}/blob/${encodeURIComponent(repository.defaultBranch)}/README.md`} {onopen} />{:else}<p class="empty-readme">暂无 README</p>{/if}
    </div>
  {/if}
</aside>

<style>
  .github-detail { min-width: 0; align-self: start; }
  .repo-detail-top { position: sticky; top: 0; z-index: 2; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 11px 16px; background: var(--neu-bg, var(--surface)); border-bottom: 1px solid var(--hairline); border-radius: calc(var(--r-card, 14px) - 1px) calc(var(--r-card, 14px) - 1px) 0 0; }
  .repo-detail-top :global(.back-arrow) { transform: rotate(180deg); }
  .detail-body { padding: 18px; min-width: 0; }
  .repo-heading { display: flex; align-items: flex-start; gap: 10px; color: var(--accent); }
  .repo-heading > :global(svg) { margin-top: 3px; }
  h2 { font-size: 20px; font-weight: 650; line-height: 1.45; overflow-wrap: anywhere; }
  .repo-flags { display: flex; flex-wrap: wrap; gap: 6px; margin: 10px 0; }
  .repo-description { color: var(--subtle); font-size: 12.5px; overflow-wrap: anywhere; line-height: 1.75; margin-top: 8px; }
  .original-name { margin-top: 4px; color: var(--muted); font-size: 12px; overflow-wrap: anywhere; }
  .local-note { margin: 10px 0 4px; padding: 9px 12px; border: 1px solid var(--hairline, var(--border)); background: var(--surface-soft); border-radius: 12px; font-size: 13.5px; line-height: 1.65; color: var(--heading); overflow-wrap: anywhere; }
  .repo-homepage { display: flex; flex-direction: column; align-items: start; gap: 3px; margin-top: 10px; font-size: 12px; overflow-wrap: anywhere; }
  .repo-homepage .text-button { white-space: normal; overflow-wrap: anywhere; display: block; }
  .repo-statline { display: flex; flex-wrap: wrap; gap: 7px 16px; color: var(--muted); font-size: 11px; margin: 15px 0 10px; font-variant-numeric: tabular-nums; }
  .repo-statline > span { display: inline-flex; align-items: center; gap: 4px; }
  .repo-topics { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0; }
  .repo-topics span { padding: 2px 9px; border-radius: 10px; background: var(--surface-soft); border: 1px solid var(--hairline, var(--border)); color: var(--subtle); font-size: 11px; overflow-wrap: anywhere; }
  .repo-actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 17px 0 8px; }
  .repo-pages { display:flex; flex-wrap:wrap; align-items:center; gap:8px 18px; margin-top:10px; }
  .release-assets { margin-top:14px; padding:12px 0; border-block:1px solid var(--hairline, var(--border)); font-size:13px; color:var(--subtle); }
  .release-assets-heading { display:flex; align-items:baseline; justify-content:space-between; flex-wrap:wrap; gap:4px 12px; }
  .release-assets-heading h3 { color:var(--heading); font-size:14px; font-weight:650; overflow-wrap:anywhere; }
  .release-assets p { overflow-wrap:anywhere; }
  .release-assets ul { margin:6px 0 0; padding:0; list-style:none; }
  .release-assets li { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:6px 12px; padding:8px 0; }
  .release-assets li + li { border-top:1px solid var(--hairline, var(--border)); }
  .asset-file { flex:1 1 90px; min-width:0; overflow-wrap:anywhere; }
  .asset-file strong { display:block; color:var(--text); font-weight:600; }
  .asset-actions { display:flex; flex-wrap:wrap; align-items:center; justify-content:flex-end; gap:8px; min-width:0; max-width:100%; margin-inline-start:auto; }
  .asset-actions button { max-width:100%; min-width:0; white-space:normal; overflow-wrap:anywhere; }
  .asset-actions :global(svg) { flex:none; }
  .asset-feedback { flex-basis:100%; margin:0; line-height:1.5; }
  .readme-heading { display: flex; align-items: center; gap: 7px; margin: 24px 0 13px; padding-bottom: 10px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .readme-heading h3 { font-size: 12px; font-weight: 600; }
  .empty-readme { color: var(--muted); font-size: 12px; }
  @media (max-height: 720px) {
    .repo-detail-top { padding: 5px 12px; gap: 8px; }
    .detail-body { padding: 10px 12px; }
    .repo-heading { gap: 7px; }
    h2 { font-size: 19px; line-height: 1.35; }
    .original-name { margin-top: 3px; font-size: 12px; }
    .repo-flags { margin: 6px 0; }
    .local-note { margin: 7px 0 4px; padding: 7px 9px; font-size: 13px; line-height: 1.6; }
  }
</style>
