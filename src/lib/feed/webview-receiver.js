/* Only this closure owns the native receiver invocation. The GitHub page never
 * receives dispatch, opener, shell, filesystem, or a local HTTP service. */
function startReleaseHubFeed(config) {
  if (window.top !== window || window.location.origin !== 'https://github.com') return;
  const bridgeName = '__releasehubFeedReader';
  if (window[bridgeName]) { window[bridgeName].request(config.requestId, 'snapshot'); return; }
  let requestId = config.requestId;
  let sequence = 0;
  let documentStarted = Date.now();
  let debounce;
  let deadline;
  let maximumWait;
  let running = false;
  let rerun = false;
  let lastFingerprint = '';
  let moreBaseline = null;
  let lastAccount = null;
  let readyAccount = null;
  let readySignature = '';
  let observerActive = false;
  let heartbeat;
  let suspended = false;
  const documentId = crypto.randomUUID();
  const invoke = window.__TAURI_INTERNALS__?.invoke;
  if (typeof invoke !== 'function') return;

  async function collect(settled = Date.now() - documentStarted > 12000) {
    if (suspended) return;
    if (running) { rerun = true; return; }
    running = true;
    const activeRequest = requestId;
    try {
      const result = readReleaseHubFeed(document, location.href, settled);
      const signature = result.items.map((item) => item.id).join('|');
      // A client-side account switch can update the account meta tag before the
      // old cards disappear. Never relabel that old snapshot as the new account.
      if (result.status === 'ready' && readyAccount && result.account !== readyAccount && signature && signature === readySignature) {
        result.items = []; result.hasMore = false;
        result.status = settled ? 'error' : 'loading';
        result.error = settled ? '账号已经切换，正在等待新账号的官网动态。请点击刷新。' : null;
      } else if (result.status === 'ready') { readyAccount = result.account; readySignature = signature; }
      if (moreBaseline !== null && result.status === 'ready' && signature === moreBaseline && result.hasMore) {
        if (settled) { result.status = 'error'; result.error = '官网尚未加载出更多动态，请重试或在官网窗口继续。'; moreBaseline = null; }
        else result.status = 'loading';
      } else if (result.status === 'ready' || result.status === 'login' || lastAccount !== result.account) moreBaseline = null;
      lastAccount = result.account;
      const { more: _control, ...page } = result;
      const fingerprint = JSON.stringify(page);
      if (fingerprint === lastFingerprint) return;
      const nextSequence = ++sequence;
      await invoke('feed_receive', { payload: { ...page, requestId: activeRequest, windowToken: config.windowToken, documentId, sequence: nextSequence } });
      if (activeRequest === requestId) lastFingerprint = fingerprint;
    } catch {
      // Do not send exceptions/page data to a network service or console. The host's
      // request timeout reports a disconnected bridge without leaking login material.
    } finally {
      running = false;
      if (rerun) { rerun = false; schedule(); }
    }
  }
  function schedule() {
    if (suspended) return;
    clearTimeout(debounce);
    debounce = setTimeout(() => { clearTimeout(maximumWait); maximumWait = undefined; void collect(); }, 600);
    if (maximumWait === undefined) maximumWait = setTimeout(() => { clearTimeout(debounce); maximumWait = undefined; void collect(); }, 2200);
  }
  function armDeadline() {
    clearTimeout(deadline);
    deadline = setTimeout(() => void collect(true), 15000);
  }
  function request(nextRequestId, action) {
    if (typeof nextRequestId !== 'string' || nextRequestId.length > 80) return;
    requestId = nextRequestId; lastFingerprint = ''; documentStarted = Date.now(); moreBaseline = null;
    if (action === 'more') {
      const current = readReleaseHubFeed(document, location.href, false);
      moreBaseline = current.items.map((item) => item.id).join('|');
      if (current.more && current.hasMore) {
        // This is an existing, recognized pagination control, never an arbitrary
        // form, Star button, settings action, or an invented endpoint.
        current.more.click();
      } else {
        moreBaseline = null;
      }
    }
    armDeadline(); void collect();
  }
  Object.defineProperty(window, bridgeName, { value: Object.freeze({ request }), writable: false, configurable: false });
  function navigationChanged() {
    documentStarted = Date.now(); moreBaseline = null; lastFingerprint = ''; armDeadline(); schedule();
  }
  for (const name of ['pushState', 'replaceState']) {
    const original = history[name];
    history[name] = function (...args) { const value = Reflect.apply(original, this, args); navigationChanged(); return value; };
  }
  addEventListener('popstate', navigationChanged);
  document.addEventListener('turbo:load', navigationChanged);
  document.addEventListener('turbo:render', schedule);
  const observer = new MutationObserver(schedule);
  const start = () => {
    if (!document.documentElement) return;
    suspended = false;
    if (!observerActive) {
      observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['content', 'aria-busy', 'disabled', 'hidden', 'aria-hidden'] });
      observerActive = true;
    }
    if (heartbeat === undefined) heartbeat = setInterval(() => void collect(), 20000);
    armDeadline(); void collect();
  };
  // A history-restored document keeps its JS closure, but pagehide suspended its
  // observer/timers. Resume them instead of leaving that page permanently stale.
  addEventListener('pageshow', () => { navigationChanged(); start(); });
  if (document.documentElement) start(); else document.addEventListener('DOMContentLoaded', start, { once: true });
  addEventListener('pagehide', () => {
    suspended = true; observer.disconnect(); observerActive = false;
    clearTimeout(debounce); clearTimeout(deadline); clearTimeout(maximumWait); maximumWait = undefined;
    clearInterval(heartbeat); heartbeat = undefined;
  });
}
