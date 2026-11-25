<script lang="ts">
  import type { DownloadRecord, GithubRepository, Project, Snapshot } from '../lib/types';
  import { formatBytes, formatDate, hasUnreadUpdate, isDownloadRunning, pendingInstallDownloads, type Page, type Tone } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import GithubFeed from '../components/GithubFeed.svelte';
  import RollingCounter from '../components/RollingCounter.svelte';
  import CardStack from '../components/CardStack.svelte';
  import CadenceModule from '../components/CadenceModule.svelte';
  let { snapshot, busy, selectedId, onpage, onselect, onadd, oncheck, ondownload, oninstall, preview = false, active = true, onopen = () => {}, onGithubAdd = async () => false, onnotice = () => {}, onGithubLogin, oninterval = () => false }: {
    snapshot: Snapshot; busy: boolean; selectedId: string | null;
    onpage: (page: Page) => void; onselect: (project: Project) => void; onadd: () => void;
    oncheck: (ids?: string[]) => void; ondownload: (project: Project) => void;
    oninstall?: (download: DownloadRecord) => void; preview?: boolean; active?: boolean;
    onopen?: (url: string) => void; onGithubAdd?: (repository: GithubRepository) => Promise<boolean>;
    onnotice?: (message: string, tone?: Tone) => void; onGithubLogin?: () => void;
    oninterval?: (minutes: number) => Promise<boolean> | boolean;
  } = $props();
  const monitored = $derived(snapshot.projects.filter(project => !project.updatesMuted));
  const updates = $derived(monitored.filter(project => hasUnreadUpdate(project, snapshot.readUpdateTags)));
  const pending = $derived(pendingInstallDownloads(snapshot.downloads, snapshot.projects));
  const activeDownloads = $derived(snapshot.downloads.filter(isDownloadRunning));
  const failures = $derived(monitored.filter((project) => project.status === 'error').length);
  const lastCheck = $derived(monitored.reduce<string | null>((latest, project) => project.checkedAt && (!latest || project.checkedAt > latest) ? project.checkedAt : latest, null));
  function showPending(download: DownloadRecord) {
    if (oninstall) oninstall(download);
    else onpage('downloads');
  }
</script>

<div class="page-heading"><div><h1>首页</h1></div><button class="button primary" disabled={busy || !monitored.length} onclick={() => oncheck()}><Icon name={busy ? 'loader' : 'refresh'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理' : '检查更新'}</button></div>

<section class="home-overview" aria-label="资源概览">
  <button onclick={() => onpage('library')}><Icon name="library" size={20} /><span class="overview-label">资源</span><RollingCounter value={snapshot.projects.length} label="资源" /></button>
  <button onclick={() => onpage('updates')}><Icon name="updates" size={20} /><span class="overview-label">未读更新</span><RollingCounter value={updates.length} label="未读更新" tone={updates.length ? 'key' : 'default'} /></button>
  <button onclick={() => onpage('downloads')}><Icon name="download" size={20} /><span class="overview-label">下载中</span><RollingCounter value={activeDownloads.length} label="下载中" tone={activeDownloads.length ? 'key' : 'default'} /></button>
</section>

{#if !snapshot.projects.length}<section class="home-first-resource"><div><h2>订阅第一个 GitHub 项目</h2><p>公开项目无需登录</p></div><button class="button primary" onclick={onadd}><Icon name="plus" size={16} />添加资源</button></section>{/if}

<CadenceModule projects={monitored} interval={snapshot.settings.checkIntervalMinutes} {busy} onsave={oninterval} {onselect} />

<div class="home-workspace-grid">
  <section class="surface" aria-labelledby="home-updates-title"><div class="section-heading"><div class="section-title-row"><h2 id="home-updates-title">版本更新</h2>{#if lastCheck}<span class="section-meta">{formatDate(lastCheck, true)}</span>{/if}</div><button class="text-button" onclick={() => onpage('updates')}>全部<Icon name="chevronRight" size={15} /></button></div>
    {#if updates.length}
      <div class="home-stack"><CardStack items={updates} key={(project) => project.id} label="版本更新卡片" onopen={onselect}>
        {#snippet card(project)}
          <div class="update-card"><strong>{project.name}</strong><p>{project.managementMode === 'files' ? '最新文件' : project.installedVersion ?? '未记录版本'}<Icon name="arrowRight" size={12} /><span class="key-data">{project.release?.tag}</span></p>{#if project.release?.publishedAt}<small>{formatDate(project.release.publishedAt)}</small>{/if}</div>
        {/snippet}
      </CardStack></div>
    {:else}<div class="home-section-empty"><Icon name="success" size={20} /><p>{snapshot.projects.length ? '没有未读更新' : '暂无订阅资源'}</p></div>{/if}
    {#if failures}<div class="section-notice"><Icon name="alert" size={15} /><span>{failures} 项检查失败</span><button class="text-button" onclick={() => onpage('library')}>查看</button></div>{/if}
  </section>

  <section class="surface" aria-labelledby="home-pending-title"><div class="section-heading"><div class="section-title-row"><h2 id="home-pending-title">待安装</h2>{#if pending.length}<span class="subtle-count">{pending.length}</span>{/if}</div><button class="text-button" onclick={() => onpage('downloads')}>全部<Icon name="chevronRight" size={15} /></button></div>
    {#each pending.slice(0, 3) as download (download.id)}<div class="home-pending-row"><div><strong>{download.projectName}</strong><p>{download.tag} · {formatBytes(download.totalBytes)}</p></div><button class="button secondary small" disabled={busy} onclick={() => showPending(download)}>安装</button></div>
    {:else}<div class="home-section-empty"><Icon name="package" size={20} /><p>{activeDownloads.length ? `${activeDownloads.length} 项下载中` : '无待安装项'}</p></div>{/each}
  </section>
</div>

<section class="surface home-feed"><GithubFeed {preview} {active} compact {onopen} onadd={onGithubAdd} {onnotice} onLogin={onGithubLogin ?? (() => onpage('github'))} /><div class="home-feed-footer"><button class="text-button" onclick={() => onpage('github')}>进入 GitHub<Icon name="arrowRight" size={15} /></button></div></section>

<style>
  .home-overview { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:20px; margin-bottom:26px; }
  .home-overview button { display:flex; align-items:center; gap:12px; padding:16px 18px; border:0; background:var(--neu-bg); color:var(--subtle); text-align:left; border-radius:var(--r-key); font-size:13px; box-shadow:var(--raise-sm); transition:transform 220ms var(--spring), box-shadow 180ms ease; }
  .home-overview button:hover { transform:translateY(-1px); box-shadow:var(--raise); }
  .home-overview button:active { transform:translateY(2px); box-shadow:var(--inset); transition-duration:120ms; }
  .home-overview .overview-label { flex:1; }
  .home-stack { padding:8px 18px 16px; }
  .update-card strong { display:block; font-size:15px; font-weight:650; color:var(--heading); overflow-wrap:anywhere; }
  .update-card p { display:flex; align-items:center; gap:6px; flex-wrap:wrap; margin-top:6px; color:var(--muted); font-size:12px; }
  .update-card small { display:block; margin-top:6px; color:var(--muted); font-size:11px; }
  .home-overview strong { color:var(--heading); font-size:20px; font-variant-numeric:tabular-nums; margin-right:4px; }
  .home-first-resource { display:flex; align-items:center; justify-content:space-between; gap:20px; margin-bottom:24px; padding:20px; border-radius:12px; background:var(--surface-soft); }
  .home-first-resource h2 { font-size:16px; margin-bottom:4px; }
  .home-first-resource p { color:var(--muted); font-size:12px; line-height:1.5; }
  .home-workspace-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:24px; margin:26px 0; align-items:start; }
  .section-title-row { display:flex; align-items:baseline; gap:10px; min-width:0; flex-wrap:wrap; }
  .section-meta { color:var(--muted); font-size:12px; font-variant-numeric:tabular-nums; }
  .home-pending-row { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:14px 18px; border-top:1px solid var(--border); }
  .home-pending-row > div { min-width:0; }
  .home-pending-row strong { font-size:14px; font-weight:600; text-align:left; overflow-wrap:anywhere; }
  .home-pending-row p { display:flex; align-items:center; gap:6px; color:var(--muted); font-size:12px; margin-top:5px; flex-wrap:wrap; overflow-wrap:anywhere; }
  .home-pending-row .button { flex-shrink:0; }
  .home-section-empty { display:flex; align-items:center; gap:10px; padding:22px 18px; color:var(--muted); border-top:1px solid var(--border); font-size:13px; line-height:1.6; }
  .home-section-empty :global(svg) { flex-shrink:0; }
  .home-feed { padding:18px; }
  .home-feed-footer { display:flex; justify-content:flex-end; margin-top:14px; border-top:1px solid var(--border); padding-top:12px; }
  @media (max-width:1100px) { .home-workspace-grid { gap:16px; } .home-overview { gap:14px; } .home-overview button { gap:8px; padding:13px 12px; } .home-pending-row { flex-wrap:wrap; } .home-first-resource { flex-wrap:wrap; } }
  @media (max-width:800px) { .home-workspace-grid { grid-template-columns:1fr; } }
</style>
