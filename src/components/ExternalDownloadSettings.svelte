<script lang="ts">
  import type { ExternalDownloader, Settings } from '../lib/types';
  import { dispatch } from '../lib/bridge';
  import { readableError } from '../lib/ui';
  import Icon from './Icon.svelte';
  import Toggle from './Toggle.svelte';
  let { settings, busy, preview, onsave }: { settings: Settings; busy: boolean; preview: boolean; onsave: (changes: Partial<Settings>) => Promise<boolean> } = $props();
  const preset: ExternalDownloader = { name: 'AB Download Manager', endpoint: 'http://127.0.0.1:15151/start-headless-download', requestTemplate: '{"downloadSource":{"link":"{url}","downloadPage":"{pageUrl}"},"name":"{fileName}","startDownload":true}' };
  let name = $state(''); let endpoint = $state(''); let template = $state('');
  let working = $state(false); let message = $state(''); let failed = $state(false); let advanced = $state(false);
  let previous = '';
  $effect(() => { const current = JSON.stringify(settings.externalDownloader ?? preset); if (current !== previous) { previous = current; const config = settings.externalDownloader ?? preset; name = config.name; endpoint = config.endpoint; template = config.requestTemplate; } });
  const draft = () => ({ name: name.trim(), endpoint: endpoint.trim(), requestTemplate: template.trim() });
  const dirty = $derived(JSON.stringify({ name: name.trim(), endpoint: endpoint.trim(), requestTemplate: template.trim() }) !== JSON.stringify(settings.externalDownloader ?? preset));
  async function save() {
    if (busy || working) return; working = true; failed = false; message = '';
    try { const ok = await onsave({ externalDownloader: draft() }); failed = !ok; message = ok ? '下载器配置已保存。' : '配置未保存，请检查上方错误提示。'; }
    catch (error) { failed = true; message = readableError(error); } finally { working = false; }
  }
  async function test() {
    if (busy || working) return; working = true; failed = false; message = '';
    try { const result = preview ? { ok: true, message: '界面预览：未连接本机下载器。' } : await dispatch('test_external_downloader', { config: draft() }); failed = !result.ok; message = result.message; }
    catch (error) { failed = true; message = readableError(error); } finally { working = false; }
  }
</script>

<section class="surface settings-section" aria-labelledby="external-download-title">
  <div class="settings-heading"><Icon name="download" size={19} /><div><h2 id="external-download-title">外部下载器</h2><p>将公开发行链接交给本机下载器</p></div></div>
  <div class="setting-row"><div><strong>手动下载默认使用外部下载器</strong><p>每次发送前确认；自动下载仍使用内置下载</p></div><Toggle checked={settings.useExternalDownloader ?? false} label="手动下载默认使用外部下载器" disabled={busy || working || dirty} onchange={() => void onsave({ useExternalDownloader: !settings.useExternalDownloader })} /></div>
  <form class="external-config" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <label for="downloader-name">下载器名称<input id="downloader-name" bind:value={name} maxlength="80" disabled={busy || working} /></label>
    <label for="downloader-endpoint">本机接口地址<input id="downloader-endpoint" bind:value={endpoint} maxlength="500" spellcheck="false" disabled={busy || working} /></label>
    <p class="field-hint">AB Download Manager：先打开下载器，在其设置中启用浏览器集成服务，默认端口为 15151。</p>
    <button type="button" class="text-button" aria-expanded={advanced} onclick={() => advanced = !advanced}>{advanced ? '收起接口请求模板' : '其他下载器？编辑接口请求模板'}<Icon name="chevronDown" size={13} /></button>
    {#if advanced}<label for="downloader-template">POST 请求内容（JSON）<textarea id="downloader-template" class="mono" bind:value={template} rows="5" maxlength="16384" spellcheck="false" disabled={busy || working}></textarea></label><p class="field-hint">占位符：{'{url}'}、{'{pageUrl}'}、{'{fileName}'}、{'{projectName}'}、{'{tag}'}。接口须是本机 HTTP 服务。</p>{/if}
    <div class="external-actions"><button type="button" class="button secondary small" disabled={busy || working} onclick={() => { name = preset.name; endpoint = preset.endpoint; template = preset.requestTemplate; message = ''; }}>填入 AB 默认配置</button><button type="button" class="button secondary small" disabled={busy || working} onclick={() => void test()}>测试端口连接</button><button class="button primary small" disabled={busy || working || !dirty}>{working ? '正在处理' : '保存下载器配置'}</button></div>
    {#if message}<p class:field-error={failed} class:field-hint={!failed} role={failed ? 'alert' : 'status'}>{message}</p>{/if}
  </form>
  <p class="setting-description">下载进度在外部下载器查看。完成后，到追新的“下载”页选择“导入已下载文件”；校验通过后即可确认安装。</p>
</section>
<style>
  .external-config { display: grid; gap: 10px; padding-top: 16px; }
  .external-config label { display: grid; gap: 7px; font-size: 12px; font-weight: 600; }
  .external-config input, .external-config textarea { width: 100%; min-width: 0; }
  .external-config .text-button { justify-self: start; }
  .external-actions { display: flex; gap: 8px; flex-wrap: wrap; }
</style>
