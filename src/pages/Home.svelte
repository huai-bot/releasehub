<script lang="ts">
  import type { DownloadRecord, GithubRepository, Project, Snapshot } from '../lib/types';
  import { formatBytes, formatDate, hasProjectUpdate, isDownloadRunning, pendingInstallDownloads, type Page, type Tone } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import GithubFeed from '../components/GithubFeed.svelte';
  let { snapshot, busy, selectedId, onpage, onselect, onadd, oncheck, ondownload, oninstall, preview = false, active = true, onopen = () => {}, onGithubAdd = async () => false, onnotice = () => {}, onGithubLogin }: {
    snapshot: Snapshot; busy: boolean; selectedId: string | null;
    onpage: (page: Page) => void; onselect: (project: Project) => void; onadd: () => void;
    oncheck: (ids?: string[]) => void; ondownload: (project: Project) => void;
    oninstall?: (download: DownloadRecord) => void; preview?: boolean; active?: boolean;
    onopen?: (url: string) => void; onGithubAdd?: (repository: GithubRepository) => Promise<boolean>;
    onnotice?: (message: string, tone?: Tone) => void; onGithubLogin?: () => void;
  } = $props();
  const updates = $derived(snapshot.projects.filter(hasProjectUpdate));
  const pending = $derived(pendingInstallDownloads(snapshot.downloads, snapshot.projects));
  const activeDownloads = $derived(snapshot.downloads.filter(isDownloadRunning));
  const failures = $derived(snapshot.projects.filter((project) => project.status === 'error').length);
  const lastCheck = $derived(snapshot.projects.reduce<string | null>((latest, project) => project.checkedAt && (!latest || project.checkedAt > latest) ? project.checkedAt : latest, null));
  function showPending(download: DownloadRecord) {
    if (oninstall) oninstall(download);
    else onpage('downloads');
  }
</script>

<div class="page-heading"><div><h1>首页</h1></div><button class="button primary" disabled={busy || !snapshot.projects.length} onclick={() => oncheck()}><Icon name={busy ? 'loader' : 'refresh'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理' : '检查更新'}</button></div>

<section class="home-overview" aria-label="资源概览">
  <button onclick={() => onpage('library')}><Icon name="library" size={20} /><span><strong>{snapshot.projects.length}</strong> 资源</span><Icon name="chevronRight" size={15} /></button>
  <button onclick={() => onpage('updates')}><Icon name="updates" size={20} /><span><strong>{updates.length}</strong> 可更新</span><Icon name="chevronRight" size={15} /></button>
  <button onclick={() => onpage('downloads')}><Icon name="download" size={20} /><span><strong>{activeDownloads.length}</strong> 下载中</span><Icon name="chevronRight" size={15} /></button>
</section>

{#if !snapshot.projects.length}<section class="home-first-resource"><div><h2>订阅第一个 GitHub 项目</h2><p>公开项目无需登录</p></div><button class="button primary" onclick={onadd}><Icon name="plus" size={16} />添加资源</button></section>{/if}

<div class="home-workspace-grid">
  <section class="surface" aria-labelledby="home-updates-title"><div class="section-heading"><div class="section-title-row"><h2 id="home-updates-title">版本更新</h2>{#if lastCheck}<span class="section-meta">{formatDate(lastCheck, true)}</span>{/if}</div><button class="text-button" onclick={() => onpage('updates')}>全部<Icon name="chevronRight" size={15} /></button></div>
    {#each updates.slice(0, 3) as project (project.id)}<div class="home-update-row"><div><button class="plain-link" onclick={() => onselect(project)}>{project.name}</button><p>{project.managementMode === 'files' ? '最新文件' : project.installedVersion ?? '未记录版本'}<Icon name="arrowRight" size={12} /><span>{project.release?.tag}</span></p></div><button class="icon-button" aria-label={`查看 ${project.name} 更新说明`} onclick={() => onselect(project)}><Icon name="notes" size={16} /></button></div>
    {:else}<div class="home-section-empty"><Icon name="success" size={20} /><p>{snapshot.projects.length ? '无待更新版本' : '暂无订阅资源'}</p></div>{/each}
    {#if failures}<div class="section-notice"><Icon name="alert" size={15} /><span>{failures} 项检查失败</span><button class="text-button" onclick={() => onpage('library')}>查看</button></div>{/if}
  </section>

  <section class="surface" aria-labelledby="home-pending-title"><div class="section-heading"><div class="section-title-row"><h2 id="home-pending-title">待安装</h2>{#if pending.length}<span class="subtle-count">{pending.length}</span>{/if}</div><button class="text-button" onclick={() => onpage('downloads')}>全部<Icon name="chevronRight" size={15} /></button></div>
    {#each pending.slice(0, 3) as download (download.id)}<div class="home-pending-row"><div><strong>{download.projectName}</strong><p>{download.tag} · {formatBytes(download.totalBytes)}</p></div><button class="button secondary small" disabled={busy} onclick={() => showPending(download)}>确认安装</button></div>
    {:else}<div class="home-section-empty"><Icon name="package" size={20} /><p>{activeDownloads.length ? `${activeDownloads.length} 项下载中` : '无待安装项'}</p></div>{/each}
  </section>
</div>

<section class="surface home-feed"><GithubFeed {preview} {active} compact {onopen} onadd={onGithubAdd} {onnotice} onLogin={onGithubLogin ?? (() => onpage('github'))} /><div class="home-feed-footer"><button class="text-button" onclick={() => onpage('github')}>进入 GitHub<Icon name="arrowRight" size={15} /></button></div></section>

<style>
  .home-overview { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); border-bottom:1px solid var(--border); margin-bottom:24px; }
  .home-overview button { display:flex; align-items:center; gap:12px; padding:17px 12px; border:0; background:none; color:var(--subtle); text-align:left; border-radius:8px; font-size:13px; }
  .home-overview button:hover { background:var(--surface-soft); }
  .home-overview button > :global(svg:last-child) { margin-left:auto; }
  .home-overview strong { color:var(--heading); font-size:20px; font-variant-numeric:tabular-nums; margin-right:4px; }
  .home-first-resource { display:flex; align-items:center; justify-content:space-between; gap:20px; margin-bottom:24px; padding:20px; border-radius:12px; background:var(--surface-soft); }
  .home-first-resource h2 { font-size:16px; margin-bottom:4px; }
  .home-first-resource p { color:var(--muted); font-size:12px; line-height:1.5; }
  .home-workspace-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:20px; margin-bottom:24px; }
  .section-title-row { display:flex; align-items:baseline; gap:10px; min-width:0; flex-wrap:wrap; }
  .section-meta { color:var(--muted); font-size:12px; font-variant-numeric:tabular-nums; }
  .home-update-row, .home-pending-row { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:14px 18px; border-top:1px solid var(--border); }
  .home-update-row > div, .home-pending-row > div { min-width:0; }
  .home-update-row .plain-link, .home-pending-row strong { font-size:14px; font-weight:600; text-align:left; overflow-wrap:anywhere; }
  .home-update-row p, .home-pending-row p { display:flex; align-items:center; gap:6px; color:var(--muted); font-size:12px; margin-top:5px; flex-wrap:wrap; overflow-wrap:anywhere; }
  .home-update-row p span { color:var(--accent); }
  .home-pending-row .button { flex-shrink:0; }
  .home-section-empty { display:flex; align-items:center; gap:10px; padding:22px 18px; color:var(--muted); border-top:1px solid var(--border); font-size:13px; line-height:1.6; }
  .home-section-empty :global(svg) { flex-shrink:0; }
  .home-feed { padding:18px; }
  .home-feed-footer { display:flex; justify-content:flex-end; margin-top:14px; border-top:1px solid var(--border); padding-top:12px; }
  @media (max-width:1100px) { .home-workspace-grid { gap:12px; } .home-overview button { gap:8px; padding:15px 8px; } .home-overview button > :global(svg:last-child) { display:none; } .home-pending-row { flex-wrap:wrap; } .home-first-resource { flex-wrap:wrap; } }
  @media (max-width:800px) { .home-workspace-grid { grid-template-columns:1fr; } }
</style>
