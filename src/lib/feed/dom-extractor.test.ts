import source from './dom-extractor.js?raw';
import { describe, expect, it } from 'vitest';
import type { GithubFeedItem } from '../types';

interface Extracted {
  status: 'loading' | 'login' | 'other' | 'ready' | 'error'; connected: boolean;
  account: string | null; items: GithubFeedItem[]; hasMore: boolean;
  sourceUrl: string; error: string | null; more: HTMLElement | null;
}
const extract = new Function(`${source}; return readReleaseHubFeed;`)() as (doc: Document, url: string, settled?: boolean) => Extracted;
const page = (html: string, account = 'sample-user') => new DOMParser().parseFromString(`<html><head>${account ? `<meta name="user-login" content="${account}">` : ''}</head><body><main>${html}</main></body></html>`, 'text/html');
function card(index: number, summary = `Release ${index}`, date = '2026-11-15T01:00:00Z') {
  return `<article id="feed-item-${index}"><header><a href="/actor" data-hovercard-type="user"><img src="https://avatars.githubusercontent.com/u/1" alt=""></a><h3>${summary}</h3></header><p>Project description ${index}</p><a href="/sample/project-${index}">sample/project-${index}</a><a href="/sample/project-${index}/releases/tag/v1"><relative-time datetime="${date}">an hour ago</relative-time></a></article>`;
}

describe('actual-page DOM extraction (synthetic website-shape fixtures, not a real login)', () => {
  it('keeps DOM order, actual dates and original links instead of sorting by time', () => {
    const result = extract(page(`${card(2, 'Older item', '2026-11-13T01:00:00Z')}${card(1, 'Newer item')}<button>Load more</button>`), 'https://github.com/dashboard-feed');
    expect(result.status).toBe('ready'); expect(result.account).toBe('sample-user');
    expect(result.items.map((item) => item.repository)).toEqual(['sample/project-2', 'sample/project-1']);
    expect(result.items[0].occurredAt).toBe('2026-11-13T01:00:00.000Z');
    expect(result.items[0].url).toBe('https://github.com/sample/project-2/releases/tag/v1');
    expect(result.items[0].avatarUrl).toBe('https://avatars.githubusercontent.com/u/1');
    expect(result.hasMore).toBe(true); expect(result.more?.textContent).toBe('Load more');
  });
  it('uses content identity, not repeated or reindexed feed-item ids or relative-time text', () => {
    const doc = page(card(0)); const first = extract(doc, 'https://github.com/dashboard-feed').items[0].id;
    doc.querySelector('article')!.id = 'feed-item-99'; doc.querySelector('relative-time')!.textContent = '2 hours ago';
    expect(extract(doc, 'https://github.com/dashboard-feed').items[0].id).toBe(first);
    doc.querySelector('h3')!.textContent = 'A different recommendation';
    expect(extract(doc, 'https://github.com/dashboard-feed').items[0].id).not.toBe(first);
  });
  it('supports the documented legacy news shape without turning arbitrary cards into feed', () => {
    const result = extract(page('<div id="dashboard"><div class="news"><div class="alert"><div class="body"><div class="title"><a href="/actor">actor</a> starred <a href="/sample/legacy">sample/legacy</a></div><p>Description</p></div></div><div class="alert" role="alert">API error</div><button class="ajax-pagination-btn">More</button></div></div>'), 'https://github.com/');
    expect(result.items).toHaveLength(1); expect(result.items[0].repository).toBe('sample/legacy'); expect(result.hasMore).toBe(true);
    expect(extract(page('<section><h3>A project card</h3><a href="/sample/not-feed">project</a></section>'), 'https://github.com/', true).status).toBe('error');
  });
  it('does not send hidden cards, form fields, scripts, external URLs or unsafe avatars', () => {
    const html = `<article id="feed-item-0"><header>Safe summary</header><p>Safe body<script>secretToken</script><form><input value="password"><span>secretForm</span></form><span hidden>hiddenSecret</span></p><a href="javascript:alert(1)">bad link</a><a href="https://github.com.evil.test/a/b">spoof</a><a href="https://user@github.com/sample/repo">userinfo</a><a href="/sample/safe">safe repo</a><img class="avatar" src="https://evil.test/a.png"></article><div hidden>${card(2)}</div>`;
    const result = extract(page(html), 'https://github.com/dashboard-feed');
    expect(result.items).toHaveLength(1); expect(result.items[0].url).toBe('https://github.com/sample/safe'); expect(result.items[0].avatarUrl).toBeNull();
    expect(result.items[0].body).not.toMatch(/secretToken|secretForm|hiddenSecret/); expect(result.items[0].repository).toBe('sample/safe');
    expect(extract(page(card(0)), 'https://github.com.evil.test/dashboard-feed').status).toBe('error');
  });
  it('bounds cards and strings while retaining more than the first 80 items', () => {
    const result = extract(page(Array.from({ length: 190 }, (_, index) => card(index, `Item ${index} ${'x'.repeat(1500)}`)).join('')), 'https://github.com/dashboard-feed');
    expect(result.items).toHaveLength(160); expect(result.items[159].repository).toBe('sample/project-159');
    expect(result.items.every((item) => item.summary.length <= 500 && item.body.length <= 1800)).toBe(true);
  });
  it('reports login, unknown markup, loading, explicit empty, and non-feed pages separately', () => {
    expect(extract(page(card(0)), 'https://github.com/login').status).toBe('login');
    expect(extract(page('', ''), 'https://github.com/dashboard-feed', true).account).toBeNull();
    expect(extract(page('<div>Unsupported markup</div>'), 'https://github.com/dashboard-feed', false).status).toBe('loading');
    expect(extract(page('<div>Unsupported markup</div>'), 'https://github.com/dashboard-feed', true).status).toBe('error');
    expect(extract(page('<p>Your feed is empty</p>'), 'https://github.com/dashboard-feed', true).status).toBe('ready');
    expect(extract(page(card(0)), 'https://github.com/sample/repository').status).toBe('other');
  });
  it('does not use Star, forms, disabled, external or in-card show-more controls as pagination', () => {
    const doc = page(`${card(0)}<button disabled>Load more</button><a href="https://evil.test/">Load more</a><button>Star</button>`);
    const inside = doc.createElement('button'); inside.textContent = 'Show more'; doc.querySelector('article')!.append(inside);
    expect(extract(doc, 'https://github.com/dashboard-feed').hasMore).toBe(false);
    expect(extract(page(`${card(0)}<a href="/dashboard-feed?page=2">Next</a>`), 'https://github.com/dashboard-feed').hasMore).toBe(true);
  });
  it('does not mistake navigation routes for repositories', () => {
    const result = extract(page('<article id="feed-item-0"><h3>Feed news</h3><a href="/settings/profile">Settings</a><a href="/orgs/sample">Organization</a></article>'), 'https://github.com/dashboard-feed');
    expect(result.items[0].repository).toBeNull(); expect(result.items[0].url).toBe('https://github.com/dashboard-feed');
  });
});
