/* Read-only DOM adapter, injected inside the GitHub window's isolated receiver closure.
 * Modern article[id^="feed-item-"] is publicly observed; legacy .news/.alert is
 * documented by old-github-feed. Fixtures model those shapes, not a captured login.
 * No Events/RSS fallback, HTML transport, cookies, forms, or internal API requests.
 */
function readReleaseHubFeed(doc, pageUrl, settled = false) {
  const MAX_ITEMS = 160;
  const reserved = new Set(['about', 'account', 'apps', 'codespaces', 'collections', 'contact', 'customer-stories', 'dashboard', 'dashboard-feed', 'enterprise', 'events', 'explore', 'features', 'issues', 'join', 'login', 'logout', 'marketplace', 'new', 'notifications', 'orgs', 'organizations', 'pricing', 'pulls', 'readme', 'search', 'security', 'sessions', 'settings', 'site', 'sponsors', 'stars', 'topics', 'trending', 'users']);
  function text(value, maximum = 1600) {
    return String(value || '').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u202a-\u202e\u2066-\u2069]/g, '').replace(/\s+/g, ' ').trim().slice(0, maximum);
  }
  function safeUrl(value, avatar = false) {
    if (!value || String(value).length > 2048) return null;
    try {
      const url = new URL(value, pageUrl);
      if (url.protocol !== 'https:' || url.username || url.password || url.port || url.hostname !== (avatar ? 'avatars.githubusercontent.com' : 'github.com')) return null;
      return url.href;
    } catch { return null; }
  }
  function visible(node) {
    return !node.closest('[hidden], [aria-hidden="true"], template') && node.getAttribute('aria-disabled') !== 'true';
  }
  function nodeText(node, maximum) {
    if (!node) return '';
    const copy = node.cloneNode(true);
    copy.querySelectorAll('script, style, noscript, svg, template, form, input, select, textarea, button, relative-time, time, [role="menu"], [hidden], [aria-hidden="true"]').forEach((child) => child.remove());
    // Separators preserve readable text even when markup puts adjacent blocks together.
    copy.querySelectorAll('p, div, h1, h2, h3, h4, li, br').forEach((child) => child.appendChild(doc.createTextNode(' ')));
    return text(copy.textContent, maximum);
  }
  function repositoryFrom(value) {
    const safe = safeUrl(value);
    if (!safe) return null;
    const path = new URL(safe).pathname.split('/').filter(Boolean);
    if (path.length < 2 || reserved.has(path[0].toLowerCase())) return null;
    return /^[a-z\d](?:[a-z\d-]{0,38})$/i.test(path[0]) && /^[a-z\d_.-]{1,100}$/i.test(path[1]) && !['.', '..'].includes(path[1]) ? `${path[0]}/${path[1]}` : null;
  }
  function profileFrom(link) {
    const safe = safeUrl(link.getAttribute('href'));
    if (!safe) return null;
    const path = new URL(safe).pathname.split('/').filter(Boolean);
    return path.length === 1 && !reserved.has(path[0].toLowerCase()) && /^[a-z\d](?:[a-z\d-]{0,38})$/i.test(path[0]) ? path[0] : null;
  }
  function hash(value) {
    let result = 2166136261;
    for (let index = 0; index < value.length; index++) result = Math.imul(result ^ value.charCodeAt(index), 16777619);
    return (result >>> 0).toString(16);
  }

  const current = safeUrl(pageUrl);
  const path = current ? new URL(current).pathname : '';
  const sourceUrl = current ? `https://github.com${path}` : 'https://github.com/dashboard-feed';
  const login = /^\/(?:login|session|sessions|logout)(?:\/|$)/.test(path) || Boolean(doc.querySelector('form[action="/session"] input[type="password"]'));
  const loginName = text(doc.querySelector('meta[name="user-login"]')?.getAttribute('content') || doc.querySelector('meta[name="octolytics-actor-login"]')?.getAttribute('content'), 80);
  const account = !login && /^[a-z\d](?:[a-z\d-]{0,38})$/i.test(loginName) ? loginName : null;
  const result = { status: 'loading', connected: Boolean(account), account, items: [], hasMore: false, sourceUrl, error: null, more: null };
  if (!current) return { ...result, status: 'error', error: '网页来源不是 GitHub 官方站点。' };
  if (login) return { ...result, status: 'login', connected: false, account: null };
  if (!['/', '/dashboard', '/dashboard-feed'].includes(path)) return { ...result, status: 'other' };
  if (!account) {
    if (!settled) return result;
    return { ...result, status: 'login', error: doc.querySelector('body.logged-in') ? '没有识别到当前网页登录账号，请在官网窗口重新打开动态页。' : null };
  }

  const roots = [...doc.querySelectorAll('feed-container, [data-target~="feed-container.content"], #dashboard .news, main .news')];
  const modern = [...doc.querySelectorAll('article[id^="feed-item-"]')].filter(visible);
  const legacy = modern.length ? [] : [...doc.querySelectorAll('#dashboard .news .alert, main .news .alert, .news > .alert')].filter((node) => visible(node) && node.querySelector('.body') && !node.matches('[role="alert"]'));
  const candidates = (modern.length ? modern : legacy).filter((node, index, all) => !all.slice(0, index).some((parent) => parent.contains(node))).slice(0, MAX_ITEMS);
  const seen = new Set();
  for (const card of candidates) {
    const links = [...card.querySelectorAll('a[href]')].filter(visible).slice(0, 120);
    const profile = links.find((link) => link.getAttribute('data-hovercard-type') === 'user' && profileFrom(link)) || links.find(profileFrom);
    const actor = profile ? text(nodeText(profile, 100) || profileFrom(profile), 100) : 'GitHub';
    const header = card.querySelector('[data-testid="feed-item-heading"], .title, header, h3, h2');
    const fullText = nodeText(card, 2400);
    const summary = nodeText(header, 500) || text(fullText, 220);
    if (!summary) continue;
    const repoLink = links.find((link) => repositoryFrom(link.getAttribute('href')));
    const repository = repoLink ? repositoryFrom(repoLink.getAttribute('href')) : null;
    const time = card.querySelector('relative-time[datetime], time[datetime]');
    const rawDate = time?.getAttribute('datetime');
    const occurredAt = rawDate && !Number.isNaN(Date.parse(rawDate)) ? new Date(rawDate).toISOString() : null;
    const detailLink = links.find((link) => {
      const safe = safeUrl(link.getAttribute('href'));
      return safe && repositoryFrom(safe) && /^\/[^/]+\/[^/]+\/(?:releases|issues|pull|discussions|commit|compare)(?:\/|$)/.test(new URL(safe).pathname);
    });
    const url = safeUrl(time?.closest('a')?.getAttribute('href')) || safeUrl(detailLink?.getAttribute('href')) || safeUrl(repoLink?.getAttribute('href')) || safeUrl(profile?.getAttribute('href')) || sourceUrl;
    const avatar = profile?.querySelector('img') || card.querySelector('img.avatar, img[src*="avatars.githubusercontent.com"]');
    const avatarUrl = safeUrl(avatar?.getAttribute('src'), true);
    // GitHub reuses feed-item-0 for different recommendations: never key on that id alone.
    const id = `feed:${hash(`${actor}\n${url}\n${summary}\n${occurredAt || ''}`)}`;
    if (seen.has(id)) continue;
    seen.add(id);
    const body = text(fullText.startsWith(summary) ? fullText.slice(summary.length) : fullText, 1800);
    result.items.push({ id, actor, avatarUrl, summary, body, url, repository, occurredAt });
  }

  const root = roots.find((node) => candidates.some((card) => node.contains(card))) || roots[0] || (path === '/dashboard-feed' ? doc.querySelector('main') || doc.body : modern[0]?.parentElement);
  if (root) {
    const controls = [...root.querySelectorAll('button, a')].filter((node) => visible(node) && !node.closest('article[id^="feed-item-"], .alert') && !node.disabled);
    const more = controls.find((node) => {
      const label = text(node.textContent, 100);
      if (!node.matches('.ajax-pagination-btn') && !/^(?:load more|show more|more activity|older activity|next|加载更多|显示更多|更多动态|下一页)(?:\s|$|[.…])/i.test(label)) return false;
      const href = node.getAttribute('href');
      if (!href || href === '#') return node.tagName === 'BUTTON' || node.matches('.ajax-pagination-btn');
      const safe = safeUrl(href);
      return Boolean(safe && ['/', '/dashboard', '/dashboard-feed'].includes(new URL(safe).pathname));
    });
    result.more = more || null;
    result.hasMore = Boolean(more);
  }
  if (result.items.length) return { ...result, status: 'ready' };
  const hasLoading = root?.querySelector('[aria-busy="true"], include-fragment[src], [role="progressbar"], .loading-spinner');
  if (hasLoading && !settled) return result;
  // A recognized empty message is required; selector drift must not become a fake empty feed.
  const emptyText = root ? nodeText(root, 2400) : '';
  if (!hasLoading && /(?:your feed is empty|no (?:recent )?activity|nothing to show|all caught up|there (?:isn't|is no) any activity|暂无动态|还没有动态|没有新的动态)/i.test(emptyText)) return { ...result, status: 'ready' };
  if (!settled) return result;
  return { ...result, status: 'error', error: '官网已打开，但没有识别到动态卡片。请打开官网窗口查看加载情况，然后重试。' };
}
