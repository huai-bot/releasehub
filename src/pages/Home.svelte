<script lang="ts">
  import type { Project, Snapshot } from '../lib/types';
  import { canInstallDownload, hasProjectUpdate, formatDate, type Page } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import ProjectList from '../components/ProjectList.svelte';
  let { snapshot, busy, selectedId, onpage, onselect, onadd, oncheck, ondownload }: {
    snapshot: Snapshot; busy: boolean; selectedId: string | null;
    onpage: (page: Page) => void; onselect: (project: Project) => void; onadd: () => void;
    oncheck: (ids?: string[]) => void; ondownload: (project: Project) => void;
  } = $props();
  const updates = $derived(snapshot.projects.filter(hasProjectUpdate));
  const recent = $derived([...snapshot.projects].sort((a, b) => (b.checkedAt ?? b.createdAt).localeCompare(a.checkedAt ?? a.createdAt)).slice(0, 5));
  const activeDownloads = $derived(snapshot.downloads.filter((download) => ['queued', 'downloading'].includes(download.status)).length);
  const pendingInstalls = $derived(snapshot.downloads.filter((download) => download.status === 'completed' && canInstallDownload(download)).length);
  const failures = $derived(snapshot.projects.filter((project) => project.status === 'error').length);
  const lastCheck = $derived(snapshot.projects.reduce<string | null>((latest, project) => project.checkedAt && (!latest || project.checkedAt > latest) ? project.checkedAt : latest, null));
</script>

<div class="page-heading"><div><h1>发行与更新</h1><p>查看订阅资源的版本、下载和发行动态。</p></div><span class="date-label">{new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', weekday: 'long' }).format(new Date())}</span></div>

<section class="overview-hero" aria-labelledby="overview-title">
  <div class="hero-copy"><h2 id="overview-title">{#if updates.length}<span>{updates.length}</span> 项资源，有了新变化{:else if snapshot.projects.length}订阅的项目，都在这里{:else}从第一个 GitHub 项目开始{/if}</h2><p>{#if updates.length}先看看更新说明，再决定要不要升级。{#if pendingInstalls}还有 {pendingInstalls} 个安装包已准备好。{/if}{:else if snapshot.projects.length}检查发行动态，查看更新说明和发行附件。{:else}粘贴项目链接，我们帮你留意正式版和预览版的发布。{/if}</p><div class="hero-actions"><button class="button primary" disabled={busy || !snapshot.projects.length} onclick={() => oncheck()}><Icon name={busy ? 'loader' : 'refresh'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理' : '检查全部更新'}</button><button class="button hero-secondary" onclick={updates.length ? () => onpage('updates') : onadd}>{updates.length ? '查看新版本' : '添加资源'}<Icon name="arrowRight" size={16} /></button></div></div>
</section>

<div class="stat-grid">
  <button class="stat-card" onclick={() => onpage('library')}><span class="stat-icon"><Icon name="library" size={20} /></span><span><span class="stat-label">已订阅资源</span><strong>{snapshot.projects.length}<small>项</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
  <button class="stat-card" onclick={() => onpage('updates')}><span class="stat-icon mint"><Icon name="updates" size={20} /></span><span><span class="stat-label">等待更新</span><strong>{updates.length}<small>项</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
  <button class="stat-card" onclick={() => onpage('downloads')}><span class="stat-icon warm"><Icon name="download" size={20} /></span><span><span class="stat-label">下载进行中</span><strong>{activeDownloads}<small>项</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
</div>

<section class="surface recent-section" aria-labelledby="recent-title"><div class="section-heading"><div><h2 id="recent-title">发行动态 <span class="subtle-count">{snapshot.projects.length}</span></h2><p>{lastCheck ? `最近检查 · ${formatDate(lastCheck, true)}` : '添加资源后，检查一次就能看到发行动态'}</p></div><button class="text-button" onclick={() => onpage('library')}>全部资源<Icon name="chevronRight" size={15} /></button></div>
  {#if recent.length}<ProjectList projects={recent} {selectedId} {busy} {onselect} {ondownload} {oncheck} />{:else}<div class="empty-state"><span class="empty-icon"><Icon name="library" size={28} /></span><h3>把常用项目放在一起</h3><p>支持公开 GitHub 项目，不登录也能开始。</p><button class="button primary small" onclick={onadd}><Icon name="plus" size={16} />添加第一个资源</button></div>{/if}
  {#if failures}<div class="section-notice"><Icon name="alert" size={16} /><span>{failures} 项资源检查失败，先看看错误原因再重试。</span><button class="text-button" onclick={() => onpage('library')}>查看</button></div>{/if}
</section>

<div class="home-bottom-grid"><button class="quiet-card" onclick={() => onpage('settings')}><span class="quiet-icon"><Icon name="drive" size={24} /></span><span><strong>你的资源库，随身带走</strong><small>导入导出订阅、备注与使用偏好。</small></span><Icon name="chevronRight" size={17} /></button><button class="quiet-card" onclick={() => onpage('agent')}><span class="quiet-icon"><Icon name="agent" size={24} /></span><span><strong>让 AI 了解你的软件近况</strong><small>通过 MCP 或命令行提供版本与问题资料。</small></span><Icon name="chevronRight" size={17} /></button></div>
