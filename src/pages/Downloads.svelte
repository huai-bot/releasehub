<script lang="ts">
  import type { DownloadRecord, InstallationCheck } from '../lib/types';
  import { assetKindLabel, canDeleteDownload, canInstallDownload, canRetryDownload, downloadPercent, formatBytes, formatDate, getDownloadStatus, isDownloadRunning } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import { canConfirmInstallation, canVerifyInstallation, installationLabel, installationSourceLabel, latestInstallation } from '../lib/installation-state';
  let { downloads, installations = [], query, busy, onretry, oncancel, oninstall, onfolder, onmark, onproject, oncopy, onversions, onDelete, onexternalretry, onexternalimport, onVerify, onConfirmInstallation }: {
    downloads: DownloadRecord[]; installations?: InstallationCheck[]; query: string; busy: boolean; onretry: (id: string) => void; oncancel: (id: string) => void;
    oninstall: (download: DownloadRecord) => void; onfolder: (id: string) => void; onmark: (id: string, version: string) => void;
    onproject: (id: string) => void; oncopy: (text: string) => void; onversions: (projectId: string) => void;
    onDelete?: (download: DownloadRecord) => void; onexternalretry?: (download: DownloadRecord) => void; onexternalimport?: (download: DownloadRecord) => void;
    onVerify?: (projectId: string) => void; onConfirmInstallation?: (id: string) => void;
  } = $props();
  let filter = $state('all');
  const retained = $derived(downloads.filter(item => item.status !== 'deleted'));
  const active = $derived(retained.filter(isDownloadRunning).length);
  const ready = $derived(retained.filter(item => ['completed', 'installed'].includes(item.status)).length);
  const visible = $derived(retained.filter(item => `${item.projectName} ${item.fileName}`.toLowerCase().includes(query.trim().toLowerCase()) && (filter === 'all' || (filter === 'active' ? isDownloadRunning(item) : filter === 'ready' ? ['completed', 'installed'].includes(item.status) : ['failed', 'cancelled'].includes(item.status)))).slice().sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
</script>

<div class="page-heading"><h1>下载<span class="heading-count">{retained.length}</span></h1></div>
<section class="surface list-surface downloads-surface">
  <div class="list-toolbar"><div class="segmented-tabs" aria-label="下载状态筛选"><button class:active={filter === 'all'} onclick={() => filter = 'all'}>全部</button><button class:active={filter === 'active'} onclick={() => filter = 'active'}>进行中 <span>{active}</span></button><button class:active={filter === 'ready'} onclick={() => filter = 'ready'}>已完成 <span>{ready}</span></button><button class:active={filter === 'issues'} onclick={() => filter = 'issues'}>失败 / 已取消</button></div></div>
  {#each visible as download (download.id)}
    {@const installable = canInstallDownload(download)}
    {@const status = getDownloadStatus(download)}
    {@const running = isDownloadRunning(download)}
    {@const installation = latestInstallation(installations, 'downloadId', download.id)}
    <article class="download-item">
      <div class="download-main"><span class="download-icon" class:download-ready={['completed', 'installed'].includes(download.status)}><Icon name={installable && download.kind === 'portable' ? 'package' : 'download'} size={23} /></span><div class="download-info"><div class="download-name"><button class="plain-link" onclick={() => onproject(download.projectId)}>{download.projectName}</button><span class="version-pill">{download.tag}</span><span class={`status-badge ${status.tone}`}>{status.label}</span></div><p class="filename" title={download.fileName}>{download.fileName}</p><div class="download-meta"><span>{download.totalBytes > 0 ? formatBytes(download.totalBytes) : '大小待确认'}</span></div></div></div>
      {#if installation}<p class="installation-status" role="status">{installationLabel(installation)}</p>{/if}
      {#if running}<div class="download-progress"><progress aria-label={`${download.projectName} 下载进度`} max="100" value={download.totalBytes > 0 ? downloadPercent(download) : undefined}></progress><div><span>{download.status === 'installing' ? '正在安装…' : download.status === 'queued' ? '等待下载' : `${formatBytes(download.receivedBytes)} / ${download.totalBytes > 0 ? formatBytes(download.totalBytes) : '总大小未知'}`}</span><strong>{download.totalBytes > 0 && download.status === 'downloading' ? `${downloadPercent(download)}%` : ''}</strong></div></div>{/if}
      {#if download.error}<div class="download-error" role="alert"><Icon name="alert" size={15} /><p>{download.error}</p></div>{/if}
      <div class="download-bottom"><details class="download-details"><summary>详情</summary><div class="download-disclosure">
        <p>{assetKindLabel(download.kind, installable, download.fileName)} · {formatDate(download.createdAt, true)}</p>
        {#if installation}
          <p>{installation.message}</p>
          {#if installation.detectedVersion}<p>本机检测版本：{installation.detectedVersion} · {installationSourceLabel(installation.source)}</p>{/if}
          <div class="installation-details">
            {#if canConfirmInstallation(installation) && onConfirmInstallation}<button class="text-button" disabled={busy} onclick={() => onConfirmInstallation?.(installation.id)}>标记已安装 {installation.expectedVersion}</button>{/if}
            {#if canVerifyInstallation(installation)}<button class="text-button" disabled={busy} onclick={() => oninstall(download)}>重新打开安装器</button>{/if}
          </div>
        {/if}
        <p>{download.verified ? '与上游摘要一致' : download.sha256 ? '已计算 SHA-256，未经上游摘要验证' : running ? '完成下载后校验' : '未记录上游校验结果'}</p>
        {#if download.sha256}<div class="hash-details"><span>SHA-256</span><code>{download.sha256}</code><button class="icon-button" aria-label="复制 SHA-256" onclick={() => oncopy(download.sha256 ?? '')}><Icon name="copy" size={15} /></button></div>{/if}
        <div class="download-more-actions">
          {#if download.status === 'external' && onexternalretry}<button class="text-button" disabled={busy} onclick={() => onexternalretry?.(download)}>重新发送到下载器</button>{/if}
          {#if installable && download.status === 'completed'}<button class="text-button" disabled={busy} onclick={() => onfolder(download.id)}>定位文件</button>{/if}
          {#if ['completed', 'installed'].includes(download.status)}<button class="text-button" disabled={busy} onclick={() => onretry(download.id)}>重新下载</button>{/if}
          {#if installable && download.status === 'installed' && download.kind === 'portable'}<button class="text-button" disabled={busy} onclick={() => onversions(download.projectId)}>管理版本</button>{/if}
          {#if !installation && installable && download.status === 'completed' && download.kind === 'installer'}<button class="text-button" disabled={busy} onclick={() => onmark(download.projectId, download.tag)}>已自行安装？记录版本</button>{/if}
          {#if onDelete && canDeleteDownload(download)}<button class="text-button danger-text" disabled={busy} onclick={() => onDelete?.(download)}>{installable ? '删除安装包' : '删除下载文件'}</button>{/if}
        </div>
      </div></details><div class="download-actions">
        {#if download.status === 'external'}<button class="button primary small" disabled={busy || !onexternalimport} onclick={() => onexternalimport?.(download)}><Icon name="import" size={14} />导入已下载文件</button>
        {:else if ['queued', 'downloading'].includes(download.status)}<button class="button secondary small" disabled={busy} onclick={() => oncancel(download.id)}>取消下载</button>
        {:else if canRetryDownload(download)}<button class="button primary small" disabled={busy} onclick={() => onretry(download.id)}>重新下载</button>
        {:else if installation && canVerifyInstallation(installation) && onVerify}<button class="button primary small" disabled={busy} onclick={() => onVerify?.(installation.projectId)}><Icon name="refresh" size={14} />立即检测</button>
        {:else if installable && download.status === 'completed'}<button class="button primary small" disabled={busy} onclick={() => oninstall(download)}><Icon name="install" size={14} />{download.kind === 'portable' ? '安装 / 更新' : '打开安装程序'}</button>
        {:else if ['completed', 'installed'].includes(download.status)}<button class="button secondary small" disabled={busy} onclick={() => onfolder(download.id)}><Icon name="folder" size={14} />定位文件</button>{/if}
      </div></div>
    </article>
  {:else}<div class="empty-state spacious"><span class="empty-icon"><Icon name="download" size={29} /></span><h3>{query ? '没有匹配的下载' : '这里暂时没有任务'}</h3>{#if filter !== 'all'}<button class="button secondary small" onclick={() => filter = 'all'}>查看全部下载</button>{/if}</div>{/each}
</section>
<style>
  .download-details { min-width: 0; flex: 1; }
  summary { color: var(--muted); font-size: 12px; cursor: pointer; width: fit-content; padding: 7px 0; }
  .download-disclosure { padding: 10px 0 2px; font-size: 12px; color: var(--muted); }
  .download-disclosure p { margin: 4px 0; }
  .download-more-actions { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 12px; }
  .hash-details { max-width: 100%; }
  .hash-details code { overflow-wrap: anywhere; min-width: 0; }
  .download-bottom { align-items: flex-start; }
  .installation-status { margin: 6px 0 10px 59px; font-size: 12px; color: var(--muted); }
  .installation-details { display: flex; flex-wrap: wrap; gap: 12px; margin: 8px 0; }
</style>
