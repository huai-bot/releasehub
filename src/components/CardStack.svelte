<script lang="ts" generics="T">
  /** Stacked cards. Drag the top card out: the cards behind sink in turn by weight
   * and spring back. Released short of the threshold it returns home; past it, the
   * card expands into its detail. Click / Enter opens it too. */
  import type { Snippet } from 'svelte';
  import { dragOutcome, prefersReducedMotion, sinkOffset } from '../lib/motion';
  import Icon from './Icon.svelte';
  let { items, key, card, onopen, label, threshold = 110 }: {
    items: T[]; key: (item: T) => string; card: Snippet<[T]>; onopen: (item: T) => void; label: string; threshold?: number;
  } = $props();
  let rotation = $state(0);
  let dx = $state(0);
  let dy = $state(0);
  let dragging = $state(false);
  let opening = $state(false);
  let startX = 0; let startY = 0; let moved = false;
  const ordered = $derived(items.length ? [...items.slice(rotation % items.length), ...items.slice(0, rotation % items.length)] : []);
  const visible = $derived(ordered.slice(0, 4));
  const progress = $derived(Math.min(1, Math.hypot(dx, dy) / threshold));
  $effect(() => { items.length; if (rotation >= items.length) rotation = 0; });
  function down(event: PointerEvent) {
    if (event.button !== 0 || opening) return;
    startX = event.clientX; startY = event.clientY; moved = false; dragging = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (!dragging) return;
    dx = event.clientX - startX; dy = event.clientY - startY;
    if (Math.hypot(dx, dy) > 4) moved = true;
  }
  function up(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const element = event.currentTarget as HTMLElement;
    if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
    if (!moved) { dx = 0; dy = 0; return; }
    if (dragOutcome(Math.hypot(dx, dy), threshold) === 'open') expand(visible[0]);
    else { dx = 0; dy = 0; }
  }
  function expand(item: T | undefined) {
    if (!item || opening) return;
    opening = true;
    setTimeout(() => { onopen(item); rotation += 1; dx = 0; dy = 0; opening = false; }, prefersReducedMotion() ? 0 : 200);
  }
  function click(event: MouseEvent) { if (moved) { moved = false; event.preventDefault(); return; } expand(visible[0]); }
  function keydown(event: KeyboardEvent) { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); expand(visible[0]); } }
</script>
<div class="card-stack" role="group" aria-label={label} style={`--stack-depth:${Math.max(0, visible.length - 1)}`}>
  {#each visible as item, depth (key(item))}
    {#if depth === 0}
      <div class="stack-card top" class:dragging class:opening role="button" tabindex="0" aria-label={`${label}：拖出或按回车查看详情`}
        style={`transform: translate(${dx}px, ${dy}px) rotate(${dx * 0.04}deg) scale(${opening ? 1.06 : dragging ? 1.02 : 1}); z-index:${10}`}
        onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onclick={click} onkeydown={keydown}>
        {@render card(item)}
      </div>
    {:else}
      <div class="stack-card rear" aria-hidden="true"
        style={`transform: translateY(${depth * 12 + sinkOffset(depth, dragging || opening ? progress : 0)}px) scale(${1 - depth * 0.045}); z-index:${10 - depth}; transition-delay:${depth * 45}ms`}>
        {@render card(item)}
      </div>
    {/if}
  {/each}
</div>
{#if items.length > 1}
  <div class="stack-footer"><span>{(rotation % items.length) + 1} / {items.length} · 拖出卡片查看详情</span><button class="icon-button" aria-label="下一张" onclick={() => rotation += 1}><Icon name="chevronRight" size={15} /></button></div>
{/if}
<style>
  .card-stack { position: relative; display: grid; padding-bottom: calc(var(--stack-depth) * 12px + 6px); }
  .stack-card { grid-area: 1 / 1; border-radius: 20px; background: var(--neu-bg); box-shadow: var(--raise); padding: 16px 18px; transform-origin: 50% 100%; touch-action: none; user-select: none;
    transition: transform 300ms var(--spring), opacity 200ms ease, box-shadow 180ms ease; }
  .stack-card.top { cursor: grab; }
  .stack-card.top:hover { box-shadow: var(--raise-hover); }
  .stack-card.top.dragging { cursor: grabbing; transition: box-shadow 180ms ease; box-shadow: var(--raise-hover), 0 18px 30px rgba(35, 32, 29, .12); }
  .stack-card.top.opening { opacity: 0; }
  .stack-card.rear { pointer-events: none; }
  .stack-card.rear :global(*) { visibility: hidden; }
  .stack-footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 10px; color: var(--muted); font-size: 12px; }
  .stack-footer .icon-button { width: 30px; height: 30px; }
</style>
