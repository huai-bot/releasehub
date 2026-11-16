<script lang="ts">
  import type { Project } from '../lib/types';
  import { canDownloadAsset, hasProjectUpdate, isFilesProject, matchesProject, needsProjectAttention } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import ProjectList from '../components/ProjectList.svelte';
  let { projects, updatesOnly = false, query, selectedId, busy, onadd, onselect, oncheck, ondownload, onbatchdownload }: {
    projects: Project[]; updatesOnly?: boolean; query: string; selectedId: string | null; busy: boolean;
    onadd: () => void; onselect: (project: Project) => void; oncheck: (ids?: string[]) => void;
    ondownload: (project: Project) => void; onbatchdownload: (projects: Project[]) => void;
  } = $props();
  let resourceType = $state<'all' | 'software' | 'files'>('all');
  let filter = $state('all');
  let selected = $state(new Set<string>());
  const base = $derived(projects.filter((project) => !updatesOnly || hasProjectUpdate(project)));
  const typed = $derived(base.filter((project) => resourceType === 'all' || (isFilesProject(project) ? resourceType === 'files' : resourceType === 'software')));
  const attention = $derived(typed.filter(needsProjectAttention).length);
  const visible = $derived(typed.filter((project) => matchesProject(project, query) && (filter === 'all' || (filter === 'attention' ? needsProjectAttention(project) : project.channel === filter))));
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((project) => selected.has(project.id)));
  const chosen = $derived(projects.filter((project) => selected.has(project.id)));
  const downloadable = $derived(chosen.filter((project) => project.release?.assets.some((asset) => asset.id === project.selectedAssetId && canDownloadAsset(asset))));
  const filtered = $derived(resourceType !== 'all' || filter !== 'all');
  $effect(() => {
    const allowed = new Set(base.map((project) => project.id));
    if ([...selected].some((id) => !allowed.has(id))) selected = new Set([...selected].filter((id) => allowed.has(id)));
  });
  function toggle(id: string) { const next = new Set(selected); if (next.has(id)) next.delete(id); else next.add(id); selected = next; }
  function toggleAll() { const next = new Set(selected); for (const project of visible) { if (allVisibleSelected) next.delete(project.id); else next.add(project.id); } selected = next; }
</script>

<div class="page-heading"><div><h1>{updatesOnly ? '可更新' : '资源库'}<span class="heading-count">{base.length}</span></h1><p>{updatesOnly ? '阅读更新说明，选择需要的软件或文件并集中下载。' : '订阅 GitHub 发行版，统一管理软件与普通文件。'}</p></div><button class="button primary" onclick={onadd}><Icon name="plus" size={17} />添加资源</button></div>

{#if updatesOnly && base.length}<div class="inline-callout"><Icon name="shieldCheck" size={19} /><div><strong>集中处理订阅更新</strong><span>软件按需安装，普通文件只保存；前往“下载”查看进度和校验结果。</span></div><button class="button small secondary" disabled={busy} onclick={() => oncheck(base.map((project) => project.id))}><Icon name="refresh" size={15} />重新检查</button></div>{/if}

<section class="surface library-surface">
  <div class="list-toolbar resource-toolbar">
    <div class="segmented-tabs" role="group" aria-label="资源类型筛选">
      <button class:active={resourceType === 'all'} aria-pressed={resourceType === 'all'} onclick={() => resourceType = 'all'}>全部<span>{base.length}</span></button>
      <button class:active={resourceType === 'software'} aria-pressed={resourceType === 'software'} onclick={() => resourceType = 'software'}>软件</button>
      <button class:active={resourceType === 'files'} aria-pressed={resourceType === 'files'} onclick={() => resourceType = 'files'}>文件</button>
    </div>
    <div class="resource-filter-actions"><label class="resource-secondary-filter">进一步筛选<select aria-label="渠道与状态筛选" bind:value={filter}><option value="all">全部渠道与状态</option><option value="stable">正式版</option><option value="preview">包含预览版</option>{#if !updatesOnly}<option value="attention">需留意{attention ? `（${attention}）` : ''}</option>{/if}</select></label><button class="icon-button" aria-label="检查列表中的资源" title="检查当前筛选结果" disabled={busy || !visible.length} onclick={() => oncheck(visible.map((project) => project.id))}><Icon name="refresh" class={busy ? 'spinning' : ''} size={17} /></button></div>
  </div>
  {#if selected.size}<div class="bulk-toolbar"><span>已选 <strong>{selected.size}</strong> 项</span><button class="text-button" disabled={busy} onclick={() => oncheck([...selected])}><Icon name="refresh" size={14} />检查更新</button><button class="text-button" disabled={busy || !downloadable.length} onclick={() => onbatchdownload(chosen)}><Icon name="download" size={14} />下载所选{#if downloadable.length !== chosen.length}<span>（{downloadable.length} 个可用）</span>{/if}</button><button class="text-button clear-selection" onclick={() => selected = new Set()}>取消选择</button></div>{/if}
  {#if visible.length}<div class="table-heading"><input type="checkbox" aria-label="选择当前显示的所有资源" checked={allVisibleSelected} indeterminate={!allVisibleSelected && visible.some((project) => selected.has(project.id))} onchange={toggleAll} /><span>资源 / 来源</span><span>版本 / 最新发行</span><span>状态</span><span></span></div><ProjectList projects={visible} {selectedId} {selected} selectable {busy} {onselect} ontoggle={toggle} {ondownload} {oncheck} />
  {:else}<div class="empty-state spacious"><span class="empty-icon"><Icon name={query || filtered ? 'search' : updatesOnly ? 'success' : 'library'} size={29} /></span><h3>{query || filtered ? '没有匹配的资源' : updatesOnly ? '当前没有待更新的资源' : '资源库还是空的'}</h3><p>{query ? '试试资源名称或“作者/仓库”，也可以缩短关键词。' : filtered ? '换一个类型或筛选条件，看看其他资源。' : updatesOnly ? '已订阅的软件和文件会出现在资源库；软件需先记录本机版本，文件下载后会自动记录状态。' : '粘贴 GitHub 项目地址，选择管理软件还是只保存文件，无需账号即可订阅。'}</p>{#if !projects.length}<button class="button primary small" onclick={onadd}><Icon name="plus" size={15} />添加资源</button>{:else if filtered}<button class="button secondary small" onclick={() => { resourceType = 'all'; filter = 'all'; }}>清除筛选</button>{:else if updatesOnly}<button class="button secondary small" disabled={busy} onclick={() => oncheck()}><Icon name="refresh" size={15} />检查全部资源</button>{/if}</div>{/if}
  <div class="list-footer"><span>显示 {visible.length} / {base.length} 项资源</span><span><Icon name="github" size={13} />发行信息来自原项目</span></div>
</section>
<p class="page-footnote"><Icon name="info" size={13} />文件来自 GitHub Releases 附件，单个文件最大 4 GB；“包含预览版”会同时检查正式版与预览版。</p>
