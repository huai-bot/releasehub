import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import App from '../../src/App.svelte';
import { filterRows, prepareRows, sortRows, type IndexReply, type IndexRequest, type IndexRow } from '../../src/lib/library-index';
import { moveLibraryOrder, reconcileLibraryOrder } from '../../src/lib/library-order';
import { createPreviewSnapshot } from '../../src/lib/preview';
import type { ManagementMode, Project, Settings } from '../../src/lib/types';

type NativePayload = { type: 'leave' } | { type: 'over'; position: { x: number; y: number } }
  | { type: 'drop'; paths: string[]; position: { x: number; y: number } };
const mocks = vi.hoisted(() => ({
  dispatch: vi.fn(),
  listenDrop: vi.fn(async (_callback: (event: { payload: NativePayload }) => void) => () => {}),
  listTrainers: vi.fn(),
}));
vi.mock('../../src/lib/bridge', () => ({ dispatch: mocks.dispatch, desktopAvailable: () => true, watchChanges: async () => () => {} }));
vi.mock('../../src/lib/trainer-catalog', () => ({
  listTrainers: mocks.listTrainers, getTrainerDetail: vi.fn(),
  openTrainerOriginal: vi.fn(), revealSavedTrainer: vi.fn(), saveTrainerAttachment: vi.fn(),
}));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: mocks.listenDrop }) }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  isMaximized: async () => false, onResized: async () => () => {}, setTheme: async () => {},
}) }));

let root: HTMLDivElement;
let cleanup: (() => Promise<void>) | undefined;
let hit: HTMLElement | null;
let hitDescriptor: PropertyDescriptor | undefined;
const baseProject = createPreviewSnapshot().projects[0];
function project(id: string, date: number, mode: ManagementMode = 'software', matches = true): Project {
  return {
    ...baseProject, id, name: id, owner: 'fixture', repo: id, managementMode: mode,
    description: matches ? 'selected' : 'excluded', createdAt: new Date(Date.UTC(2025, 0, date)).toISOString(),
  };
}
function fixture(projects: Project[], settings: Partial<Settings> = {}) {
  const database = createPreviewSnapshot();
  database.projects = projects;
  database.settings = { ...database.settings, startupPage: 'library', ...settings };
  mocks.dispatch.mockImplementation(async (action: string, payload: { settings?: Partial<Settings> }) => {
    if (action === 'snapshot') return structuredClone(database);
    if (action === 'save_settings') {
      Object.assign(database.settings, payload.settings);
      return structuredClone(database.settings);
    }
    if (action === 'add_local_executables') return { added: [], skipped: [] };
    throw new Error(`Unexpected action ${action}`);
  });
  return database;
}
function library() { return root.querySelector<HTMLElement>('#main-content')!; }
function cell(id: string) { return library().querySelector<HTMLElement>(`[data-order-id="${id}"]`)!; }
function handle(id: string) { return cell(id).querySelector<HTMLButtonElement>('.order-handle')!; }
function visibleOrder() { return [...library().querySelectorAll<HTMLElement>('[data-order-id]')].map(el => el.dataset.orderId); }
function button(label: string, content: ParentNode = root) {
  const result = [...content.querySelectorAll<HTMLButtonElement>('button')].find(el => el.getAttribute('aria-label') === label || el.textContent?.trim() === label);
  if (!result) throw new Error(`Missing ${label}`);
  return result;
}
async function settle() { for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); } }
async function start() { const instance = mount(App, { target: root }); cleanup = () => unmount(instance); await settle(); }
async function stop() { await cleanup?.(); cleanup = undefined; }
async function click(label: string, content?: ParentNode) { button(label, content).click(); await settle(); }
async function pickSort(label: string) { await click('资源排序'); await click(label, root.querySelector('[role=listbox]')!); }
async function search(value: string) {
  const input = root.querySelector<HTMLInputElement>('input[aria-label="搜索资源库"]')!;
  input.value = value; input.dispatchEvent(new Event('input', { bubbles: true })); await settle();
}
async function tab(kind: '软件' | '文件') {
  const button = [...library().querySelectorAll<HTMLButtonElement>('[aria-label="资源类型筛选"] button')].find(el => el.textContent?.startsWith(kind))!;
  button.click(); await settle();
}
async function key(element: HTMLElement, key: string) {
  element.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })); await settle();
}
function pointer(element: EventTarget, type: string, x: number, y: number) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y });
  Object.defineProperties(event, { pointerId: { value: 1 }, isPrimary: { value: true } });
  element.dispatchEvent(event);
}
function drop(payload: NativePayload) { mocks.listenDrop.mock.calls[0][0]({ payload }); }
function writes() { return mocks.dispatch.mock.calls.filter(([action]) => action === 'save_settings'); }

beforeEach(() => {
  vi.clearAllMocks(); cleanup = undefined; hit = null;
  root = document.createElement('div'); document.body.append(root);
  hitDescriptor = Object.getOwnPropertyDescriptor(document, 'elementFromPoint');
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => hit });
  vi.stubGlobal('matchMedia', () => ({ matches: true, addEventListener() {}, removeEventListener() {} }));
});
afterEach(async () => {
  await stop(); root.remove();
  if (hitDescriptor) Object.defineProperty(document, 'elementFromPoint', hitDescriptor); else Reflect.deleteProperty(document, 'elementFromPoint');
  vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks();
});

describe('manual library ordering', () => {
  it('keeps surviving ranks, removes missing and duplicate IDs, and appends newly added items', () => {
    expect(reconcileLibraryOrder(['newest', 'new', 'older', 'oldest'], ['oldest', 'removed', 'older', 'oldest'])).toEqual(['oldest', 'older', 'newest', 'new']);
    expect(moveLibraryOrder(['a', 'hidden', 'b', 'c'], ['a', 'b', 'c'], 'c', 'a', 'before'))
      .toEqual({ order: ['c', 'hidden', 'a', 'b'], index: 0 });
    expect(moveLibraryOrder(['a', 'hidden', 'b'], ['a', 'b'], 'a', 'b', 'before')).toBeNull();
  });

  it('keeps hidden slots and independent tabs through keyboard, menus, alternate sorts and reopen', async () => {
    const database = fixture([project('s1', 10), project('hidden', 9, 'software', false), project('s2', 8), project('f1', 7, 'files'), project('f2', 6, 'files')]);
    await start(); expect(library().querySelector('.order-handle')).toBeNull();
    await pickSort('手动排序'); expect(visibleOrder()).toEqual(['s1', 'hidden', 's2']);
    await search('selected'); expect(visibleOrder()).toEqual(['s1', 's2']);
    handle('s2').focus(); await key(handle('s2'), 'ArrowUp');
    expect(visibleOrder()).toEqual(['s2', 's1']);
    expect(document.activeElement).toBe(handle('s2'));
    expect(library().querySelector('[role=status]')?.textContent).toContain('已移至第 1 项');
    expect(writes().at(-1)).toEqual(['save_settings', { settings: { librarySoftwareOrder: ['s2', 'hidden', 's1'] } }]);
    await search(''); expect(visibleOrder()).toEqual(['s2', 'hidden', 's1']);
    await tab('文件'); handle('f1').click(); await settle();
    const menu = root.querySelector('[role=menu]')!;
    expect(button('上移一位', menu).disabled).toBe(true);
    expect(document.activeElement).toBe(button('下移一位', menu));
    await click('下移一位', menu);
    expect(visibleOrder()).toEqual(['f2', 'f1']);
    expect(writes().at(-1)).toEqual(['save_settings', { settings: { libraryFilesOrder: ['f2', 'f1'] } }]);
    await pickSort('最近添加'); expect(library().querySelector('.order-handle')).toBeNull();
    await pickSort('手动排序'); expect(visibleOrder()).toEqual(['f2', 'f1']);
    database.projects = [project('new', 11), ...database.projects];
    await stop(); await start();
    expect(visibleOrder()).toEqual(['s2', 'hidden', 's1', 'new']);
    await tab('文件'); expect(visibleOrder()).toEqual(['f2', 'f1']);
  });

  it('moves by keyboard across the page boundary and restores focus to the moved item', async () => {
    fixture(Array.from({ length: 50 }, (_, index) => project(`s${index + 1}`, 100 - index)), { librarySort: 'manual' });
    await start(); expect(visibleOrder()).toHaveLength(48);
    handle('s48').focus(); await key(handle('s48'), 'ArrowDown');
    expect(visibleOrder()).toEqual(['s48', 's50']);
    expect(document.activeElement).toBe(handle('s48'));
    expect(library().querySelector('[aria-label="手动排序分页"]')?.textContent).toContain('2 / 2');
    await key(handle('s48'), 'ArrowUp');
    expect(visibleOrder().at(-1)).toBe('s48');
    expect(document.activeElement).toBe(handle('s48'));
  });

  it('keeps a drag across pages using the current worker results, never a stale search response', async () => {
    class FixtureWorker {
      static current: FixtureWorker;
      requests: IndexRequest[] = [];
      rows: IndexRow[] = [];
      onmessage?: (event: { data: IndexReply }) => void;
      constructor() { FixtureWorker.current = this; }
      postMessage(request: IndexRequest) { this.requests.push(request); if (request.rows) this.rows = prepareRows(request.rows); }
      reply(request: IndexRequest) { this.onmessage?.({ data: { generation: request.generation, result: filterRows(sortRows(this.rows, request.sort), request.query, request.includeUnfiltered) } }); }
      terminate() {}
    }
    vi.stubGlobal('Worker', FixtureWorker);
    const software = Array.from({ length: 122 }, (_, index) => project(`s${index + 1}`, 150 - index, 'software', index < 51 && index !== 1));
    const database = fixture(software, { librarySort: 'manual' });
    await start(); const worker = FixtureWorker.current;
    const initial = worker.requests.at(-1)!; worker.reply(initial); await settle();
    await search('selected'); const searchRequest = worker.requests.at(-1)!;
    expect(handle('s1').getAttribute('aria-disabled')).toBe('true');
    await key(handle('s1'), 'ArrowDown'); expect(writes()).toHaveLength(0);
    worker.reply(searchRequest); await settle();
    worker.reply(initial); await settle();
    expect(visibleOrder().slice(0, 3)).toEqual(['s1', 's3', 's4']);

    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    pointer(handle('s1'), 'pointerdown', 5, 5);
    hit = button('下一页'); pointer(window, 'pointermove', 160, 30); await settle();
    expect(library().querySelector('.order-rail')?.textContent).toContain('正在移动：s1');
    await vi.advanceTimersByTimeAsync(540); await settle();
    expect(visibleOrder()).toEqual(['s50', 's51']);
    hit = cell('s50'); pointer(window, 'pointermove', 180, 100); pointer(window, 'pointerup', 180, 100); await settle();
    expect(visibleOrder()).toEqual(['s1', 's51']);
    expect(database.settings.librarySoftwareOrder?.slice(0, 4)).toEqual(['s3', 's2', 's4', 's5']);
    expect(database.settings.librarySoftwareOrder?.slice(48, 52)).toEqual(['s50', 's1', 's51', 's52']);
    await search(''); worker.reply(worker.requests.at(-1)!); await settle();
    expect(visibleOrder()[1]).toBe('s2');
  });

  it('cancels reordering for a native EXE drop and handles that drop only on the software tab', async () => {
    fixture([project('s1', 3), project('s2', 2), project('f1', 1, 'files')], { librarySort: 'manual' });
    await start();
    vi.spyOn(library(), 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 400, 300));
    vi.stubGlobal('devicePixelRatio', 2);
    const position = { x: 600, y: 300 }; // Native coordinates are physical pixels.
    const paths = ['C:\\Fixture\\portable.exe'];
    pointer(handle('s1'), 'pointerdown', 5, 5); hit = cell('s2'); pointer(window, 'pointermove', 50, 50);
    await settle(); expect(library().querySelector('.order-after')).toBe(cell('s2'));
    drop({ type: 'over', position }); await settle();
    expect(library().textContent).toContain('松开以添加本地软件');
    expect(library().querySelector('.order-after')).toBeNull();
    pointer(window, 'pointerup', 50, 50); drop({ type: 'drop', position, paths }); await settle();
    expect(writes()).toHaveLength(0); expect(visibleOrder()).toEqual(['s1', 's2']);
    expect(mocks.dispatch).toHaveBeenCalledWith('add_local_executables', { paths });
    await tab('文件'); drop({ type: 'drop', position, paths }); await settle();
    expect(mocks.dispatch.mock.calls.filter(([action]) => action === 'add_local_executables')).toHaveLength(1);
  });

  it('does not let an older in-flight snapshot undo a saved reorder', async () => {
    const database = fixture([project('s1', 2), project('s2', 1)], { librarySort: 'manual' });
    await start();
    const stale = structuredClone(database);
    let release!: () => void;
    mocks.dispatch.mockImplementationOnce(() => new Promise(resolve => { release = () => resolve(stale); }));
    await click('刷新本机状态');
    await key(handle('s2'), 'ArrowUp'); expect(visibleOrder()).toEqual(['s2', 's1']);
    release(); await settle();
    expect(visibleOrder()).toEqual(['s2', 's1']);
  });
});

it('retains the trainer search and scroll after visiting the library, including its Ctrl+K shortcut', async () => {
  fixture([]);
  mocks.listTrainers.mockResolvedValue({
    items: [{ title: 'Fixture North Trainer', url: 'https://flingtrainer.com/trainer/fixture-north/', publishedAt: null, updatedAt: null, summary: '' }],
    page: 1, totalPages: 1, hasNextPage: false,
  });
  const navigate = async (page: string) => {
    root.querySelector<HTMLButtonElement>(`.nav-item[data-page="${page}"]`)!.click();
    await settle();
  };
  await start(); expect(mocks.listTrainers).not.toHaveBeenCalled();
  await navigate('trainers');
  const trainer = root.querySelector<HTMLElement>('.trainer-workspace')!;
  const input = trainer.querySelector('input')!;
  input.value = 'North'; input.dispatchEvent(new Event('input', { bubbles: true }));
  trainer.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle();
  expect(mocks.listTrainers).toHaveBeenLastCalledWith('search', 'North', 1, false);
  trainer.scrollTop = 180;
  await navigate('library'); expect(trainer.hidden).toBe(true);
  await navigate('trainers');
  expect(trainer.hidden).toBe(false);
  expect(trainer.scrollTop).toBe(180);
  expect(trainer.querySelector('input')).toBe(input);
  expect(input.value).toBe('North');
  expect(mocks.listTrainers).toHaveBeenCalledTimes(2);
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, cancelable: true })); await settle();
  expect(document.activeElement).toBe(input);
});
