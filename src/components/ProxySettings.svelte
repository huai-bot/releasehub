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
  <div class="settings-heading"><Icon name="connected" size={19} /><div><h2 id="proxy-title">网络代理</h2><p>用于 GitHub 连接、检查更新和下载</p></div></div>
  <form onsubmit={save}>
    <div class="setting-row"><div><label for="proxy-mode">连接方式</label><p>更改后应用于新请求；正在下载的文件可取消后重新下载</p></div><select id="proxy-mode" bind:value={mode} disabled={pending}><option value="system">跟随系统</option><option value="manual">手动设置</option><option value="direct">不使用代理</option></select></div>
    {#if mode === 'manual'}
      <div class="proxy-fields">
        <label for="proxy-url">代理地址<input id="proxy-url" bind:value={url} placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080" spellcheck="false" required maxlength="1000" disabled={pending} /></label>
        <div class="form-columns"><label>账号 <span class="optional">可选</span><input bind:value={username} autocomplete="off" maxlength="200" disabled={pending} placeholder="代理服务的账号" /></label><label>密码 <span class="optional">可选</span><input type="password" bind:value={password} autocomplete="new-password" maxlength="1024" disabled={pending || clearPassword} placeholder={passwordConfigured ? '已保存；留空沿用' : '代理服务的密码'} /></label></div>
        {#if passwordConfigured}<label class="password-clear"><input type="checkbox" bind:checked={clearPassword} disabled={pending} />清除已保存的代理密码</label>{/if}
        <p class="field-hint">支持 HTTP 与 SOCKS5。账号和密码填写在单独的输入框中。</p>
      </div>
    {:else if mode === 'system'}<p class="setting-description proxy-description">读取 Windows 的手动代理设置或当前启动环境的代理地址。使用自动配置脚本时，请在这里填写对应的本机代理端口。</p>
    {:else}<p class="setting-description proxy-description">GitHub 请求和文件下载直接连接网络。</p>{/if}
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
    {#if result}<div class:success-text={result.ok} class:danger-text={!result.ok} class="proxy-result" role="status"><Icon name={result.ok ? 'success' : 'alert'} size={16} /><span>{result.message}{result.ok ? ` · ${result.latencyMs} ms` : ''}</span></div>{/if}
    <div class="settings-bottom-line"><span class="field-hint">{preview ? '预览中的设置仅用于展示' : '本机 AI 连接始终直连'}</span><div class="proxy-actions"><button type="button" class="button secondary small" disabled={pending || preview} onclick={() => void test()}><Icon name={testing ? 'loader' : 'connected'} class={testing ? 'spinning' : ''} size={15} />{testing ? '正在测试' : '测试连接'}</button><button type="submit" class="button primary small" disabled={pending || !dirty}>{saving ? '正在保存' : '保存代理设置'}</button></div></div>
  </form>
</section>

<style>
  .proxy-fields { display: flex; flex-direction: column; gap: 12px; padding: 16px 0; }
  .proxy-fields label { display: flex; flex-direction: column; gap: 6px; color: var(--subtle); }
  .proxy-fields .password-clear { flex-direction: row; align-items: center; font-size: 12px; }
  .proxy-description { margin: 12px 0; }
  .proxy-actions { display: flex; gap: 8px; }
  .proxy-result { display: flex; gap: 8px; align-items: center; margin: 12px 0; overflow-wrap: anywhere; }
</style>
