import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Trainers from '../src/pages/Trainers.svelte';
import LibrarySortMenu from '../src/components/LibrarySortMenu.svelte';
import type { TrainerEntry, TrainerDetail, TrainerListPage } from '../src/lib/trainer-catalog';

const api = vi.hoisted(() => ({ listTrainers: vi.fn(), getTrainerDetail: vi.fn(), saveTrainerAttachment: vi.fn(), sendTrainerToDownloader: vi.fn(), openTrainerOriginal: vi.fn(), revealSavedTrainer: vi.fn() }));
vi.mock('../src/lib/trainer-catalog', () => api);
let root: HTMLDivElement;
let cleanup: (() => Promise<void>) | null;
const game: TrainerEntry = { title: 'Archive North Trainer', url: 'https://flingtrainer.com/trainer/mock-north/', publishedAt: null, updatedAt: '2026-11-07T12:17:00Z', summary: '版本 1.2' };
const attachment = { label: 'North v1.2', url: 'https://flingtrainer.com/downloads/mock-file' };
const item: TrainerDetail = { ...game, content: '来自官网的纯文本\n<img src=x onerror="x">', attachments: [attachment] };
const first: TrainerListPage = { items: [game], page: 1, totalPages: 2, hasNextPage: true };
async function flush() { for (let i = 0; i < 10; i++) { await Promise.resolve(); flushSync(); } }
function startCatalog(externalDownloaderName: string | null = null) { const instance = mount(Trainers, { target: root, props: { preview: false, externalDownloaderName } }); cleanup = () => unmount(instance); }
function btn(label: string) { const result = [...root.querySelectorAll<HTMLButtonElement>('button')].find(el => el.getAttribute('aria-label') === label || el.textContent?.trim() === label); expect(result, label).toBeDefined(); return result!; }
async function click(label: string) { btn(label).click(); await flush(); }
async function term(query: string) { const input = root.querySelector<HTMLInputElement>('input')!; input.value = query; input.dispatchEvent(new Event('input', { bubbles: true })); await flush(); root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true })); await flush(); }
function key(el: HTMLElement, key: string) { el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })); }
function later<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => resolve = yes); return { promise, resolve }; }
beforeEach(() => { vi.resetAllMocks(); root = document.createElement('div'); document.body.append(root); cleanup = null; api.listTrainers.mockResolvedValue(first); api.getTrainerDetail.mockResolvedValue(item); });
afterEach(async () => { await cleanup?.(); root.remove(); });

describe('local FLiNG catalog', () => {
  it('renders only local controls, navigates pages and searches the official catalog', async () => {
    startCatalog(); await flush(); expect(api.listTrainers).toHaveBeenCalledWith('latest', '', 1, false);
    expect(root.querySelector('iframe, webview')).toBeNull(); expect(root.textContent).toContain('2026.11.07');
    await click('全部修改器'); expect(api.listTrainers).toHaveBeenLastCalledWith('all', '', 1, false);
    await click('下一页'); expect(api.listTrainers).toHaveBeenLastCalledWith('all', '', 2, false);
    await term('  North  '); expect(api.listTrainers).toHaveBeenLastCalledWith('search', 'North', 1, false);
    await click('清除搜索'); expect(api.listTrainers).toHaveBeenLastCalledWith('all', '', 1, false);
  });
  it('preserves search and the focused row on return, and never injects remote markup', async () => {
    startCatalog(); await flush(); await term('North'); await click(`查看 ${game.title}`);
    expect(root.textContent).toContain(item.content); expect(root.querySelector('img')).toBeNull();
    await click('官网原文'); expect(api.openTrainerOriginal).toHaveBeenCalledWith(game.url);
    await click('返回搜索结果'); expect(api.listTrainers).toHaveBeenCalledTimes(2);
    expect(document.activeElement).toBe(btn(`查看 ${game.title}`)); expect(root.querySelector('input')?.value).toBe('North');
  });
  it('distinguishes Save As cancellation from a saved path and bind saves to this entry', async () => {
    startCatalog(); await flush(); await click(`查看 ${game.title}`);
    api.saveTrainerAttachment.mockResolvedValueOnce({ status: 'cancelled' }).mockResolvedValueOnce({ status: 'saved', path: 'C:\\example\\North.zip' });
    await click(`另存为 ${attachment.label}`); expect(root.textContent).toContain('已取消');
    await click(`另存为 ${attachment.label}`); expect(api.saveTrainerAttachment).toHaveBeenLastCalledWith(game.url, attachment.url);
    expect(root.textContent).toContain('C:\\example\\North.zip'); expect(root.textContent).not.toContain('已取消');
    await click('在文件夹中显示'); expect(api.revealSavedTrainer).toHaveBeenCalledWith('C:\\example\\North.zip');
  });
  it.each(['sent', 'unknown'] as const)('hands off once, distinguishes %s from a downloaded file, and keeps Save As', async status => {
    startCatalog('AB Download Manager'); await flush(); await click(`查看 ${game.title}`);
    const label = `将 ${attachment.label} 发送到 AB Download Manager`;
    const pending = later<{ status: typeof status }>(); api.sendTrainerToDownloader.mockReturnValueOnce(pending.promise);
    btn(label).click(); btn(label).click(); await flush();
    expect(api.sendTrainerToDownloader).toHaveBeenCalledTimes(1); expect(api.sendTrainerToDownloader).toHaveBeenCalledWith(game.url, attachment.url);
    expect(api.saveTrainerAttachment).not.toHaveBeenCalled(); pending.resolve({ status }); await flush();
    expect(root.textContent).toContain(status === 'sent' ? '请在下载器中查看进度' : '请先检查下载器队列');
    expect(btn(label).disabled).toBe(true); expect(root.textContent).not.toContain('在文件夹中显示');
    await click('返回目录'); await click(`查看 ${game.title}`); expect(btn(label).disabled).toBe(true);
    api.saveTrainerAttachment.mockResolvedValueOnce({ status: 'cancelled' }); await click(`另存为 ${attachment.label}`); expect(api.saveTrainerAttachment).toHaveBeenCalledOnce();
  });
  it('allows a user-initiated retry after explicit refusal without falling back automatically', async () => {
    startCatalog('自定义下载器'); await flush(); await click(`查看 ${game.title}`);
    const label = `将 ${attachment.label} 发送到 自定义下载器`;
    api.sendTrainerToDownloader.mockRejectedValueOnce(new Error('本机接口未连接')); await click(label);
    expect(root.textContent).toContain('本机接口未连接'); expect(btn(label).disabled).toBe(false); expect(api.saveTrainerAttachment).not.toHaveBeenCalled();
    api.sendTrainerToDownloader.mockResolvedValueOnce({ status: 'sent' }); await click(label); expect(root.textContent).toContain('已交给 自定义下载器');
  });
  it('keeps a newer result when an older request finishes late and shows a recoverable empty state', async () => {
    const slow = later<TrainerListPage>(); api.listTrainers.mockReturnValueOnce(slow.promise).mockResolvedValueOnce({ ...first, items: [], hasNextPage: false });
    startCatalog(); await flush(); await term('unknown'); slow.resolve(first); await flush();
    expect(root.textContent).toContain('没有找到匹配的游戏'); expect(root.querySelector('.game-row')).toBeNull();
    await click('返回目录'); expect(root.querySelector('.game-row')).not.toBeNull();
  });
  it('keeps the working list after a failed refresh and retries a failed detail request', async () => {
    startCatalog(); await flush(); api.listTrainers.mockRejectedValueOnce(new Error('官网暂不可用。')); await click('刷新目录');
    expect(api.listTrainers).toHaveBeenLastCalledWith('latest', '', 1, true); expect(root.textContent).toContain('仍显示上次的目录'); expect(root.textContent).toContain(game.summary);
    api.getTrainerDetail.mockRejectedValueOnce(new Error('未能读取详情。')); await click(`查看 ${game.title}`);
    expect(root.textContent).toContain('未能读取详情'); await click('重试'); expect(root.textContent).toContain('官方附件');
    expect(api.getTrainerDetail).toHaveBeenLastCalledWith(game.url, true);
    api.getTrainerDetail.mockRejectedValueOnce(new Error('官网暂不可用。')); await click('刷新详情');
    expect(root.textContent).toContain('仍显示上次的详情'); expect(root.textContent).toContain(item.content);
  });
});

describe('library sort popup', () => {
  function show() { const onsort = vi.fn(); const instance = mount(LibrarySortMenu, { target: root, props: { sort: 'added', onsort } }); cleanup = () => unmount(instance); return onsort; }
  it('offers all six orders, marks only the current one and accepts a pointer choice', async () => {
    const changed = show(); await click('资源排序'); expect(root.querySelectorAll('[role=option]')).toHaveLength(6);
    expect(root.querySelector('[role=option][aria-selected=true]')?.textContent?.trim()).toBe('最近添加');
    await click('手动排序'); expect(changed).toHaveBeenCalledWith('manual'); expect(root.querySelector('[role=listbox]')).toBeNull();
  });
  it('keeps focus on the control, navigates by keyboard, and cancels without committing', async () => {
    const changed = show(); const combo = btn('资源排序'); combo.focus(); key(combo, 'ArrowDown'); await flush();
    key(combo, 'End'); await flush(); const id = combo.getAttribute('aria-activedescendant')!; expect(document.getElementById(id)?.textContent?.trim()).toBe('可更新优先');
    key(combo, 'Escape'); await flush(); expect(changed).not.toHaveBeenCalled(); expect(document.activeElement).toBe(combo);
    key(combo, 'Home'); key(combo, 'Enter'); await flush(); expect(changed).toHaveBeenCalledWith('manual');
    combo.click(); await flush(); document.body.dispatchEvent(new Event('pointerdown', { bubbles: true, composed: true })); await flush(); expect(combo.getAttribute('aria-expanded')).toBe('false');
  });
});
