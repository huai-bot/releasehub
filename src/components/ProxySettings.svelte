<script lang="ts">
  import type { ProxySettings, ProxyTestResult } from '../lib/types';
  import { readableError } from '../lib/ui';
  import Icon from './Icon.svelte';
  let { settings, passwordConfigured, busy, preview, onsave, ontest }: {
    settings: ProxySettings; passwordConfigured: boolean; busy: boolean; preview: boolean;
    onsave: (proxy: ProxySettings, password?: string) => Promise<boolean>;
    ontest: (proxy: ProxySettings, password?: string) => Promise<ProxyTestResult>;
  } = $props();
  let mode = $state<ProxySettings['mode']>('system');
  let url = $state('');
  let username = $state('');
  let password = $state('');
  let clearPassword = $state(false);
  let testing = $state(false);
  let saving = $state(false);
  let error = $state('');
  let result = $state<ProxyTestResult | null>(null);
  let savedKey = '';
  $effect(() => {
    const next = JSON.stringify(settings);
    if (savedKey !== next) { savedKey = next; mode = settings.mode; url = settings.url; username = settings.username; password = ''; clearPassword = false; result = null; }
  });
  const pending = $derived(busy || testing || saving);
  const draft = $derived<ProxySettings>({ mode, url: url.trim(), username: username.trim() });
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(settings) || !!password || clearPassword);
  const passwordValue = () => clearPassword ? '' : password || undefined;
  const retryTime = $derived.by(() => {
    if (!result?.retryAt) return '';
    const date = new Date(result.retryAt);
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (pending) return; error = ''; saving = true;
    try { if (await onsave(draft, passwordValue())) { password = ''; clearPassword = false; result = null; } }
    catch (cause) { error = readableError(cause); }
    finally { saving = false; }
  }
  async function test() {
    if (pending) return; error = ''; result = null; testing = true;
    try { result = await ontest(draft, passwordValue()); }
    catch (cause) { error = readableError(cause); }
    finally { testing = false; }
  }
</script>

<section class="surface settings-section" aria-labelledby="proxy-title">
  <div class="settings-heading"><h2 id="proxy-title">网络代理</h2></div>
  <form onsubmit={save}>
    <div class="setting-row"><label for="proxy-mode">连接方式</label><select id="proxy-mode" bind:value={mode} disabled={pending}><option value="system">跟随系统</option><option value="manual">手动设置</option><option value="direct">不使用代理</option></select></div>
    {#if mode === 'manual'}
      <div class="proxy-fields">
        <label for="proxy-url">代理地址<input id="proxy-url" bind:value={url} placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080" spellcheck="false" required maxlength="1000" disabled={pending} /></label>
        <div class="form-columns"><label>账号<input bind:value={username} autocomplete="off" maxlength="200" disabled={pending} placeholder="代理账号" /></label><label>密码<input type="password" bind:value={password} autocomplete="new-password" maxlength="1024" disabled={pending || clearPassword} placeholder={passwordConfigured ? '已保存，留空沿用' : '代理密码'} /></label></div>
        {#if passwordConfigured}<label class="password-clear"><input type="checkbox" bind:checked={clearPassword} disabled={pending} />清除已保存的代理密码</label>{/if}
      </div>
    {/if}
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
    {#if result}<div class="proxy-feedback" role="status"><div class:success-text={result.ok} class:danger-text={!result.ok} class="proxy-result"><Icon name={result.ok ? 'success' : 'alert'} size={16} /><span>{result.message}{result.ok ? ` · ${result.latencyMs} ms` : ''}</span></div>{#if result.warning}<p class="proxy-warning">{result.warning}{#if retryTime}<span>预计 {retryTime} 恢复。</span>{/if}</p>{/if}</div>{/if}
    <div class="settings-bottom-line"><span class="field-hint">{preview ? '界面预览' : '本机 AI 连接始终直连'}</span><div class="proxy-actions"><button type="button" class="button secondary small" disabled={pending || preview} onclick={() => void test()}><Icon name={testing ? 'loader' : 'connected'} class={testing ? 'spinning' : ''} size={15} />{testing ? '正在测试' : '测试连接'}</button><button type="submit" class="button primary small" disabled={pending || !dirty}>{saving ? '正在保存' : '保存代理设置'}</button></div></div>
  </form>
</section>

<style>
  .proxy-fields { display: flex; flex-direction: column; gap: 12px; padding: 16px 0; }
  .proxy-fields label { display: flex; flex-direction: column; gap: 6px; color: var(--subtle); }
  .proxy-fields .password-clear { flex-direction: row; align-items: center; font-size: 12px; }
  .proxy-actions { display: flex; gap: 8px; }
  .proxy-feedback { margin: 12px 0; }
  .proxy-result { display: flex; gap: 8px; align-items: center; overflow-wrap: anywhere; }
  .proxy-warning { margin: 7px 0 0; color: var(--warning); font-size: 12px; line-height: 1.65; overflow-wrap: anywhere; }
  .proxy-warning span { margin-left: 6px; }
</style>
