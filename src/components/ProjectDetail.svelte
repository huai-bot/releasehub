<script lang="ts">
  import type { AssetPreference, BackupRecord, Channel, InstallationCheck, ManagementMode, Project, ProjectChanges, RepositoryNote } from '../lib/types';
  import { assetKindLabel, canDownloadAsset, canInstallAsset, formatBytes, formatDate, getProjectStatus, hasProjectUpdate, isFilesProject, projectInitials, projectPresentation } from '../lib/ui';
  import Icon from './Icon.svelte';
  import { canConfirmInstallation, canVerifyInstallation, installationLabel, installationSourceLabel, latestInstallation } from '../lib/installation-state';
  import ReleaseNotes from './ReleaseNotes.svelte';
  let { project, readTag, note = null, installations = [], busy, onclose, oncheck, ondownload, onupdate, onmark, onremove, onopen, onversions, ondiagnostic, preview = false, onrun, onexecutable, onrestore, ondeletebackup, onexternal, onnote, onSettings, onVerify, onConfirmInstallation, initialTab = 'notes' }: {
    readTag?: string;
    project: Project; note?: RepositoryNote | null; installations?: InstallationCheck[]; busy: boolean; onclose: () => void; oncheck: (ids: string[]) => void;
    ondownload: (project: Project) => void; onexternal?: (project: Project) => void; onupdate: (id: string, changes: ProjectChanges) => void;
    onmark: (project: Project) => void;
    onremove: (project: Project) => void; onopen: (url: string) => void; onversions: (projectId: string) => void;
    ondiagnostic: (projectId: string) => void; preview?: boolean; initialTab?: 'notes' | 'manage';
    onrun: (project: Project) => void; onexecutable: (project: Project) => void;
    onrestore: (backup: BackupRecord) => void; ondeletebackup: (backup: BackupRecord) => void;
    onnote?: (project: Project) => void;
    onSettings: (project: Project) => void;
    onVerify?: (projectId: string) => void; onConfirmInstallation?: (id: string) => void;
  } = $props();
  const presentation = $derived(projectPresentation(project, note));
  let tab = $state<'notes' | 'assets'>('notes');
  let previousId = $state<string | null>(null);
  $effect(() => { if (previousId !== project.id) { previousId = project.id; tab = initialTab === 'manage' ? 'notes' : initialTab; } });
  let panel = $state<HTMLElement>();
  $effect(() => { project.id; if (panel) panel.scrollTop = 0; });
  const files = $derived(isFilesProject(project));
  const installation = $derived(files ? null : latestInstallation(installations, 'projectId', project.id));
  const status = $derived(getProjectStatus(project));
  const chosenAsset = $derived(project.release?.assets.find((asset) => asset.id === project.selectedAssetId));
  const selectedAsset = $derived(chosenAsset && canDownloadAsset(chosenAsset) ? chosenAsset : undefined);
</script>

<aside bind:this={panel} class="detail-panel" aria-label={`${presentation.label} 详情`}>
  <div class="detail-topline"><button class="icon-button" aria-label="关闭资源详情" onclick={onclose}><Icon name="close" size={18} /></button></div>
  <div class="detail-identity">
    <span class="app-avatar large" class:avatar-purple={project.repo.toLowerCase() === 'kazumi'} class:avatar-blue={project.repo.toLowerCase() === 'apkstart'}>{projectInitials(project)}</span>
    {#if presentation.title}
      <h2>{presentation.title}</h2>
    {/if}
    <button class="text-button muted" onclick={() => onopen(project.url)}>{project.owner}/{project.repo}<Icon name="external" size={13} /></button>
    {#if presentation.description}
      <p>{presentation.description}</p>
    {/if}
    {#if !project.updatesMuted && !(readTag && readTag === project.release?.tag && hasProjectUpdate(project) && project.status === 'updateAvailable')}
      <span class={`status-badge ${status.tone}`}><span class="tiny-dot"></span>{status.label}</span>
    {/if}
    {#if onnote}
      <button class="text-button secondary small" disabled={busy} onclick={() => onnote?.(project)}>
        <Icon name="edit" size={13} />{presentation.annotated ? '编辑备注' : '设置备注'}
      </button>
    {/if}
  </div>
  <div class="version-summary"><div><span>{files ? '管理方式' : '已记录版本'}</span><strong>{files ? '仅保存文件' : project.installedVersion ?? '尚未记录'}</strong>{#if !files && project.installedSource}<small>{project.installedSource === 'managed' ? '由追新管理' : project.installedSource === 'installer' ? '已自动验证' : '手动确认'}</small>{/if}</div><Icon name="arrowRight" size={18} /><div><span>最新发行 <em class:preview-label={project.release?.prerelease}>{project.release?.prerelease ? '预览' : '正式'}</em></span><strong class="accent-text">{project.release?.tag ?? '暂无信息'}</strong>{#if project.release?.publishedAt}<small>{formatDate(project.release.publishedAt)}</small>{/if}</div></div>
  {#if installation}
    <div class="installation-box">
      <p role="status">{installationLabel(installation)}</p>
      <details><summary>安装检测详情</summary>
        <p>{installation.message}</p>
        {#if installation.detectedVersion}<p>本机检测版本：{installation.detectedVersion} · {installationSourceLabel(installation.source)}</p>{/if}
        {#if canConfirmInstallation(installation) && onConfirmInstallation}<button class="text-button" disabled={busy} onclick={() => onConfirmInstallation?.(installation.id)}>标记已安装 {installation.expectedVersion}</button>{/if}
      </details>
    </div>
  {/if}
  <div class="detail-primary-actions">{#if installation && canVerifyInstallation(installation) && onVerify}<button class="button primary" disabled={busy} onclick={() => onVerify?.(project.id)}><Icon name="refresh" size={16} />立即检测</button>{:else}<button class="button primary" disabled={busy || !selectedAsset} onclick={() => ondownload(project)}><Icon name="download" size={16} />下载已选文件</button>{/if}{#if !project.updatesMuted && (!installation || !canVerifyInstallation(installation) || !onVerify)}<button class="button secondary" disabled={busy} aria-label={`检查 ${presentation.label} 更新`} onclick={() => oncheck([project.id])}><Icon name="refresh" size={16} /></button>{/if}</div>
  {#if !files}<div class="managed-entry"><button class="button secondary small" disabled={busy} onclick={() => onrun(project)}><Icon name="play" size={15} />{project.executablePath ? '打开软件' : '选择启动程序'}</button><button class="text-button" disabled={busy} onclick={() => onmark(project)}>修改本机版本</button></div>{/if}
  {#if project.error}<div class="detail-error" role="alert"><Icon name="alert" size={16} /><span>{project.error}</span></div>{/if}
  {#if project.release && !selectedAsset}<button class="asset-needed" onclick={() => tab = 'assets'}><Icon name="info" size={16} /><span>选择发行文件</span><Icon name="chevronRight" size={14} /></button>{/if}
  <div class="detail-tabs" role="tablist" aria-label="详情内容"><button role="tab" aria-selected={tab === 'notes'} class:active={tab === 'notes'} onclick={() => tab = 'notes'}>更新说明</button><button role="tab" aria-selected={tab === 'assets'} class:active={tab === 'assets'} onclick={() => tab = 'assets'}>发行文件 <span>{project.release?.assets.length ?? 0}</span></button></div>
  <div class="detail-content">
    {#if tab === 'notes'}
      {#if project.release}<div class="release-title"><span>{project.release.name || project.release.tag}</span><button class="icon-button" title="在 GitHub 阅读完整说明" aria-label="在 GitHub 阅读完整说明" onclick={() => onopen(project.release!.url)}><Icon name="external" size={15} /></button></div><ReleaseNotes body={project.release.body} repositoryUrl={project.release.url} {onopen} />{:else}<div class="empty-state compact"><Icon name="notes" size={30} /><h3>{project.status === 'noRelease' ? '尚无发行版' : '暂无发行信息'}</h3>{#if !project.updatesMuted}<button class="button secondary small" disabled={busy} onclick={() => oncheck([project.id])}>检查更新</button>{/if}</div>{/if}
      {#if project.sourceAhead}<div class="source-callout"><Icon name="code" size={17} /><div><strong>源码有未发布提交</strong><button class="text-button" onclick={() => onopen(project.url)}>查看原项目<Icon name="external" size={13} /></button></div></div>{/if}
    {:else if tab === 'assets'}
      <p class="field-hint">{files ? '仅保存附件，不自动安装 · 单文件 ≤ 4 GB' : '仅兼容软件包支持安装 · 单文件 ≤ 4 GB'}</p>
      <div class="asset-list">
        {#each project.release?.assets ?? [] as asset (asset.id)}
          {@const downloadable = canDownloadAsset(asset)}
          <article class="asset-choice" class:chosen={project.selectedAssetId === asset.id}>
            <span class="asset-choice-icon"><Icon name={project.selectedAssetId === asset.id ? 'success' : 'download'} size={19} /></span>
            <span><strong>{asset.name}</strong><small>{formatBytes(asset.size)}</small></span>
            <div class="asset-actions"><button class="text-button" disabled={busy || !downloadable} aria-pressed={project.selectedAssetId === asset.id} aria-label={`将 ${asset.name} 选为默认下载文件`} onclick={() => onupdate(project.id, { selectedAssetId: asset.id })}>{project.selectedAssetId === asset.id ? '默认文件' : '选为默认'}</button><button class="button secondary small" disabled={busy || !downloadable} aria-label={`下载文件 ${asset.name}`} onclick={() => ondownload({ ...project, selectedAssetId: asset.id })}><Icon name="download" size={14} />下载此文件</button></div>
          </article>
        {:else}<div class="empty-state compact"><Icon name="package" size={27} /><h3>暂无可下载的发行文件</h3></div>{/each}
      </div>
    {/if}
  </div>
  <div class="detail-footer"><button class="button secondary" onclick={() => onSettings(project)}><Icon name="settings" size={14} />项目设置</button></div>
</aside>

<style>
  .detail-topline { justify-content: flex-end; }
  .managed-entry { display: flex; flex-direction: column; gap: 6px; margin: 0 23px 14px; }
  .detail-footer { padding: 14px 23px; border-top: 1px solid var(--border); text-align: center; }
  .installation-box { margin: 0 23px 14px; font-size: 12px; color: var(--muted); }
  .installation-box summary { cursor: pointer; padding: 6px 0; }
  .installation-box details > p { line-height: 1.6; overflow-wrap: anywhere; }
  .installation-box button { margin-top: 6px; }
  .installation-box p { margin: 2px 0; }
</style>
