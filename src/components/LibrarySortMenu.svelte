<script lang="ts">
  import { tick } from 'svelte';
  import type { LibrarySort } from '../lib/types';
  import Icon from './Icon.svelte';

  let { sort, onsort }: { sort: LibrarySort; onsort: (value: LibrarySort) => void } = $props();
  const options: { value: LibrarySort; label: string }[] = [
    { value: 'manual', label: '手动排序' }, { value: 'added', label: '最近添加' },
    { value: 'name', label: '名称升序' }, { value: 'nameDesc', label: '名称降序' },
    { value: 'released', label: '最近发布' }, { value: 'updated', label: '可更新优先' },
  ];
  const uid = $props.id();
  const listId = `${uid}-sort-list`;
  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let open = $state(false);
  let cursor = $state(0);
  const selected = $derived(Math.max(0, options.findIndex(item => item.value === sort)));

  function expand(index = selected) { cursor = index; open = true; }
  function pick(index: number) {
    open = false;
    trigger.focus({ preventScroll: true });
    if (options[index].value !== sort) onsort(options[index].value);
  }
  function outside(e: PointerEvent) { if (open && !e.composedPath().includes(root)) open = false; }
  function key(e: KeyboardEvent) {
    if (e.altKey || e.metaKey || e.ctrlKey) return;
    if (e.key === 'Tab') { open = false; return; }
    if (e.key === 'Escape') {
      if (open) { e.preventDefault(); e.stopPropagation(); open = false; } return;
    }
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (open) pick(cursor); else expand(); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    if (e.key === 'Home') expand(0);
    else if (e.key === 'End') expand(options.length - 1);
    else if (!open) expand();
    else cursor = Math.max(0, Math.min(options.length - 1, cursor + (e.key === 'ArrowDown' ? 1 : -1)));
    void tick().then(() => root?.querySelector('[data-cursor="true"]')?.scrollIntoView?.({ block: 'nearest' }));
  }
</script>

<svelte:window onpointerdown={outside} onblur={() => open = false} />
<div class="sort-picker" bind:this={root}>
  <span class="sort-caption" aria-hidden="true">排序</span>
  <button class="sort-key" type="button" role="combobox" aria-label="资源排序" aria-haspopup="listbox" aria-expanded={open} aria-controls={listId}
    aria-activedescendant={open ? `${listId}-${cursor}` : undefined} bind:this={trigger}
    onkeydown={key} onblur={() => open = false} onclick={() => { if (open) open = false; else expand(); }}>
    <span>{options[selected].label}</span><Icon name="chevronDown" size={15} />
  </button>
  {#if open}
    <div class="sort-popover" id={listId} role="listbox" aria-label="排序方式">
      {#each options as item, i (item.value)}
        <button type="button" role="option" id={`${listId}-${i}`} tabindex="-1" aria-selected={item.value === sort}
          data-cursor={i === cursor} onpointermove={() => cursor = i} onpointerdown={e => e.preventDefault()} onclick={() => pick(i)}>
          {#if i === 0}<Icon name="grip" size={15} />{/if}<span>{item.label}</span>
          {#if item.value === sort}<Icon name="check" size={15} />{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .sort-picker { position:relative; display:inline-flex; align-items:center; gap:9px; flex:none; font-size:13px; }
  .sort-caption { color:var(--muted); font-size:12px; }
  .sort-key { display:flex; align-items:center; justify-content:space-between; gap:12px; min-height:38px; min-width:108px; padding:0 13px; border:0; border-radius:var(--r-key); background:var(--neu-bg); color:var(--text); font:inherit; font-weight:600; box-shadow:var(--raise-sm); transition:box-shadow 180ms ease, transform 200ms var(--spring), color 160ms ease; }
  .sort-key:hover { box-shadow:var(--raise); }
  .sort-key:active, .sort-key[aria-expanded="true"] { color:var(--cobalt); box-shadow:var(--inset); transform:translateY(2px); }
  .sort-key :global(svg) { flex:none; transition:transform 200ms var(--spring); }
  .sort-key[aria-expanded="true"] :global(svg) { transform:rotate(180deg); }
  .sort-popover { position:absolute; inset-inline-end:0; top:calc(100% + 9px); z-index:30; width:172px; max-height:min(340px,52vh); overflow-y:auto; scrollbar-width:thin; padding:8px; background:var(--neu-bg); border-radius:var(--r-card); box-shadow:var(--raise), 0 10px 23px color-mix(in srgb, var(--neu-dark) 65%, transparent); transform-origin:top right; animation:sort-open 180ms var(--spring-soft); }
  .sort-popover button { width:100%; display:flex; align-items:center; gap:6px; padding:0 12px; min-height:36px; border-radius:var(--r-key); text-align:left; color:var(--text); font:inherit; }
  .sort-popover button span { flex:1; }
  .sort-popover button:first-child { margin-bottom:8px; position:relative; }
  .sort-popover button:first-child::after { content:''; position:absolute; left:12px; right:12px; bottom:-4px; border-bottom:1px solid var(--hairline); }
  .sort-popover button[data-cursor="true"] { background:var(--quiet-hover); }
  .sort-popover button[aria-selected="true"] { font-weight:650; background:var(--neu-bg); color:var(--cobalt); box-shadow:var(--inset-sm); }
  .sort-key:focus-visible { outline:2px solid var(--focus); outline-offset:3px; }
  .sort-picker:has(.sort-key:focus-visible) button[data-cursor="true"] { outline:1px solid var(--muted); outline-offset:-2px; }
  @keyframes sort-open { from { opacity:.6; transform:translateY(-4px) scale(.98); } to { opacity:1; transform:none; } }
  @media (forced-colors: active) { .sort-key, .sort-popover { border:1px solid ButtonText; } .sort-popover button[aria-selected="true"] { color:HighlightText; background:Highlight; } .sort-popover button[data-cursor="true"] { outline:2px solid Highlight; } }
</style>
