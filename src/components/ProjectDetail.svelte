<script lang="ts">
  import type { AssetPreference, BackupRecord, Channel, ManagementMode, Project, ProjectChanges, RepositoryNote } from '../lib/types';
  import { assetKindLabel, canDownloadAsset, canInstallAsset, formatBytes, formatDate, getProjectStatus, isFilesProject, projectInitials, projectPresentation } from '../lib/ui';
  import Icon from './Icon.svelte';
  import ReleaseNotes from './ReleaseNotes.svelte';
  import BackupPanel from './BackupPanel.svelte';
  let { project, note = null, busy, onclose, oncheck, ondownload, onupdate, onmark, onremove, onopen, onversions, ondiagnostic, preview = false, onrun, onexecutable, onrestore, ondeletebackup, onexternal, onnote, initialTab = 'notes' }: {
    project: Project; note?: RepositoryNote | null; busy: boolean; onclose: () => void; oncheck: (ids: string[]) => void;
    ondownload: (project: Project) => void; onexternal?: (project: Project) => void; onupdate: (id: string, changes: ProjectChanges) => void;
    onmark: (project: Project) => void;
    onremove: (project: Project) => void; onopen: (url: string) => void; onversions: (projectId: string) => void;
    ondiagnostic: (projectId: string) => void; preview?: boolean; initialTab?: 'notes' | 'manage';
    onrun: (project: Project) => void; onexecutable: (project: Project) => void;
    onrestore: (backup: BackupRecord) => void; ondeletebackup: (backup: BackupRecord) => void;
    onnote?: (project: Project) => void;
  } = $props();
  const presentation = $derived(projectPresentation(project, note));
  let tab = $state<'notes' | 'assets' | 'manage'>('notes');
  let previousId = $state<string | null>(null);
  $effect(() => { if (previousId !== project.id) { previousId = project.id; tab = initialTab; } });
  let panel = $state<HTMLElement>();
  $effect(() => { project.id; if (panel) panel.scrollTop = 0; });
  const files = $derived(isFilesProject(project));
  const status = $derived(getProjectStatus(project));
  const chosenAsset = $derived(project.release?.assets.find((asset) => asset.id === project.selectedAssetId));
  const selectedAsset = $derived(chosenAsset && canDownloadAsset(chosenAsset) ? chosenAsset : undefined);
  const canManageSoftware = $derived(!files && (!chosenAsset || canInstallAsset(chosenAsset, project)));
</script>

<aside bind:this={panel} class="detail-panel" aria-label={`${presentation.label} 详情`}>
  <div class="detail-topline"><span>{files ? '文件订阅详情' : '软件详情'}</span><button class="icon-button" aria-label="关闭资源详情" onclick={onclose}><Icon name="close" size={18} /></button></div>
  <div class="detail-identity">
    <span class="app-avatar large" class:avatar-purple={project.repo.toLowerCase() === 'kazumi'} class:avatar-blue={project.repo.toLowerCase() === 'apkstart'}>{projectInitials(project)}</span>
    {#if presentation.title}
      <h2>{presentation.title}</h2>
    {/if}
    <button class="text-button muted" onclick={() => onopen(project.url)}>{project.owner}/{project.repo}<Icon name="external" size={13} /></button>
    {#if presentation.description}
      <p>{presentation.description}</p>
    {/if}
    <span class={`status-badge ${status.tone}`}><span class="tiny-dot"></span>{status.label}</span>
    {#if onnote}
      <button class="text-button secondary small" disabled={busy} onclick={() => onnote?.(project)}>
        <Icon name="edit" size={13} />{presentation.annotated ? '编辑备注' : '设置备注'}
      </button>
    {/if}
  </div>
  <div class="version-summary"><div><span>{files ? '管理方式' : '已记录版本'}</span><strong>{files ? '只保存文件' : project.installedVersion ?? '尚未记录'}</strong><small>{files ? '下载后查看位置与校验结果' : project.installedSource === 'managed' ? '由追新管理' : project.installedSource === 'manual' ? '手动确认' : '可手动记录或交给 AI 识别'}</small></div><Icon name="arrowRight" size={18} /><div><span>最新发行 <em class:preview-label={project.release?.prerelease}>{project.release?.prerelease ? '预览' : '正式'}</em></span><strong class="accent-text">{project.release?.tag ?? '暂无信息'}</strong><small>{formatDate(project.release?.publishedAt ?? null)}</small></div></div>
  <div class="detail-primary-actions"><button class="button primary" disabled={busy || !selectedAsset} onclick={() => ondownload(project)}><Icon name="download" size={16} />下载已选文件</button><button class="button secondary" disabled={busy} aria-label={`检查 ${presentation.label} 更新`} onclick={() => oncheck([project.id])}><Icon name="refresh" size={16} /></button></div>
  {#if !files}<div class="managed-entry"><button class="button secondary small" disabled={busy} onclick={() => onrun(project)}><Icon name="play" size={15} />{project.executablePath ? '打开软件' : '选择启动程序'}</button><span>{project.executablePath ?? '可直接选择主程序 EXE，无需先选择安装目录'}</span></div>{/if}
  {#if canManageSoftware && project.installedSource === 'managed'}<div class="managed-entry"><button class="button secondary small" disabled={busy} onclick={() => onversions(project.id)}><Icon name="drive" size={15} />管理已准备版本</button><span>打开程序目录，或确认切换当前记录</span></div>{/if}
  {#if project.error}<div class="detail-error"><Icon name="alert" size={16} /><span>{project.error}</span></div>{/if}
  {#if project.release && !selectedAsset}<button class="asset-needed" onclick={() => tab = 'assets'}><Icon name="info" size={16} /><span>先查看发行文件，选择要保存的附件。</span><Icon name="chevronRight" size={14} /></button>{/if}
  {#if onexternal}<div class="managed-entry"><button class="button secondary small" disabled={busy || !selectedAsset} onclick={() => onexternal?.(project)}><Icon name="external" size={14} />交给外部下载器</button></div>{/if}
  <div class="detail-tabs" role="tablist" aria-label="详情内容"><button role="tab" aria-selected={tab === 'notes'} class:active={tab === 'notes'} onclick={() => tab = 'notes'}>更新说明</button><button role="tab" aria-selected={tab === 'assets'} class:active={tab === 'assets'} onclick={() => tab = 'assets'}>发行文件 <span>{project.release?.assets.length ?? 0}</span></button><button role="tab" aria-selected={tab === 'manage'} class:active={tab === 'manage'} onclick={() => tab = 'manage'}>管理</button></div>
  <div class="detail-content">
    {#if tab === 'notes'}
      {#if project.release}<div class="release-title"><span>{project.release.name || project.release.tag}</span><button class="icon-button" title="在 GitHub 阅读完整说明" aria-label="在 GitHub 阅读完整说明" onclick={() => onopen(project.release!.url)}><Icon name="external" size={15} /></button></div><ReleaseNotes body={project.release.body} repositoryUrl={project.release.url} {onopen} />{:else}<div class="empty-state compact"><Icon name="notes" size={30} /><h3>{project.status === 'noRelease' ? '作者还没有发布发行版' : '还没有发行信息'}</h3><p>检查更新以获取作者发布的版本，或复制资料给 AI 处理源码。</p><button class="button secondary small" disabled={busy} onclick={() => oncheck([project.id])}>检查发行动态</button></div>{/if}
      {#if project.sourceAhead}<div class="source-callout"><Icon name="code" size={17} /><div><strong>源码有尚未发行的变化</strong><p>可查看源码变化，或复制资料交给外部 Agent 编译。</p><button class="text-button" onclick={() => onopen(project.url)}>查看原项目<Icon name="external" size={13} /></button></div></div>{/if}
    {:else if tab === 'assets'}
      <p class="field-hint">{files ? '可分别下载同一版本的多个附件，文件只保存，不会安装或自动打开。' : '可下载任意符合限制的附件；只有识别为适合此设备的软件包才提供安装。'} 单个文件最大 4 GB。</p>
      <div class="asset-list">
        {#each project.release?.assets ?? [] as asset (asset.id)}
          {@const downloadable = canDownloadAsset(asset)}
          {@const installable = canInstallAsset(asset, project)}
          <article class="asset-choice" class:chosen={project.selectedAssetId === asset.id}>
            <span class="asset-choice-icon"><Icon name={project.selectedAssetId === asset.id ? 'success' : installable ? 'package' : 'download'} size={19} /></span>
            <span><strong>{asset.name}</strong><small>{formatBytes(asset.size)} · {assetKindLabel(asset.kind, installable, asset.name)}</small><em>{downloadable ? installable ? '可下载 · 可安装，请核对文件名与平台' : '可下载 · 仅保存文件' : asset.reason || '此文件无法下载，请到原项目查看'}</em></span>
            <div class="asset-actions"><button class="text-button" disabled={busy || !downloadable} aria-pressed={project.selectedAssetId === asset.id} aria-label={`将 ${asset.name} 选为默认下载文件`} onclick={() => onupdate(project.id, { selectedAssetId: asset.id })}>{project.selectedAssetId === asset.id ? '默认文件' : '选为默认'}</button><button class="button secondary small" disabled={busy || !downloadable} aria-label={`下载文件 ${asset.name}`} onclick={() => ondownload({ ...project, selectedAssetId: asset.id })}><Icon name="download" size={14} />下载此文件</button></div>
          </article>
        {:else}<div class="empty-state compact"><Icon name="package" size={27} /><h3>没有可下载的发行文件</h3><p>可前往原项目查看，或复制资料交给外部 Agent 编译。</p></div>{/each}
      </div>
      {#if selectedAsset}<div class="asset-integrity"><Icon name="shield" size={15} /><span>{selectedAsset.digest ? '下载后会比对上游提供的摘要，结果见下载页。' : '此文件未提供上游摘要，下载后可在下载页查看文件校验值。'}</span></div>{/if}
    {:else}
      <div class="detail-settings">
        <label>管理方式<select value={project.managementMode ?? 'software'} disabled={busy} onchange={(event) => onupdate(project.id, { managementMode: event.currentTarget.value as ManagementMode })}><option value="software">软件 · 管理安装与版本</option><option value="files">文件 · 只下载和保存</option></select></label>
        <p class="field-hint">{files ? '只跟踪文件的下载与更新，不显示安装操作。' : '识别软件包并保留安装确认；普通附件仍可单独下载。'}</p>
        <label>订阅渠道<select value={project.channel} disabled={busy} onchange={(event) => onupdate(project.id, { channel: event.currentTarget.value as Channel })}><option value="stable">正式版 · 更稳妥</option><option value="preview">包含预览版 · 更早体验</option></select></label>
        {#if !files}<label>安装包偏好<select value={project.preference} disabled={busy} onchange={(event) => onupdate(project.id, { preference: event.currentTarget.value as AssetPreference })}><option value="auto">自动推荐，歧义时手选</option><option value="portable">优先免安装包</option><option value="installer">优先安装程序</option></select></label>{/if}
        <p class="field-hint">更改管理方式、渠道或偏好后，会重新检查已发布的版本。</p>
        <form onsubmit={(event) => { event.preventDefault(); const value = String(new FormData(event.currentTarget).get('pattern') ?? '').trim(); onupdate(project.id, { assetPattern: value || null }); }}><label>文件名筛选 <span class="optional">可选</span><input name="pattern" value={project.assetPattern ?? ''} placeholder={files ? '例如 .pdf 或资料名称' : '例如 win-x64'} maxlength="200" disabled={busy} /></label><div class="form-inline-end"><button class="button secondary small" disabled={busy} type="submit">保存筛选</button></div></form>
        {#if canManageSoftware}<div class="detail-divider"></div><div class="setting-mini"><strong>本机版本记录</strong><p>{project.installedPath ?? '没有记录安装位置'}</p><button class="button secondary small" disabled={busy} onclick={() => onmark(project)}><Icon name="check" size={14} />手动记录已安装版本</button></div>{/if}
        {#if !files}<div class="setting-mini"><strong>启动程序</strong><p>{project.executablePath ?? '尚未选择'}</p><button class="button secondary small" disabled={busy} onclick={() => onexecutable(project)}>选择或更换 EXE</button></div><p class="field-hint">支持直接定位程序 EXE，无需预先设定工作目录或安装路径。</p><BackupPanel {preview} projectId={project.id} {busy} {onrestore} ondelete={ondeletebackup} />{/if}
        <div class="detail-divider"></div><button class="text-button danger-text" disabled={busy} onclick={() => onremove(project)}><Icon name="trash" size={15} />取消订阅此资源</button><p class="field-hint">移除订阅，保留下载记录与已保存的文件。</p>
      </div>
    {/if}
  </div>
  <div class="diagnostic-entry"><button class="button secondary small" disabled={busy} onclick={() => ondiagnostic(project.id)}><Icon name="agent" size={15} />复制资料给 AI</button><span>{files ? '发行版本、文件与报错记录' : '版本、源码、安装位置与报错记录'}</span></div>
  <div class="detail-footer"><Icon name="clock" size={13} />最近检查 · {formatDate(project.checkedAt, true)}</div>
</aside>

<style>
  .diagnostic-entry { display: flex; flex-direction: column; gap: 5px; padding: 14px 23px; border-top: 1px solid var(--border); }
  .diagnostic-entry span { color: var(--muted); font-size: 11px; text-align: center; }
  .managed-entry { display: flex; flex-direction: column; gap: 6px; margin: 0 23px 14px; }
  .managed-entry span { color: var(--muted); font-size: 10px; text-align: center; }
</style>
