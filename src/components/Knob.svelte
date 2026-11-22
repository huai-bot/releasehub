<script lang="ts">
  /** Soft-UI rotary knob. Drag around it (or use arrow keys); it detents on ticks with
   * a spring, the outer ring lights the selected range, and `oninput` reports every
   * detent so linked numbers and chart target lines follow live. */
  import { angleToTick, arcPath, indexToAngle, nearestTick, pointerAngle, KNOB_START } from '../lib/motion';
  let { values, value, label, format = (item: number) => String(item), disabled = false, oninput = () => {}, onchange }: {
    values: readonly number[]; value: number; label: string; format?: (item: number) => string; disabled?: boolean;
    oninput?: (item: number) => void; onchange: (item: number) => void;
  } = $props();
  let live = $state<number | null>(null);
  let pending = $state<number | null>(null);
  let dragging = $state(false);
  let element = $state<HTMLDivElement>();
  let commitTimer: ReturnType<typeof setTimeout> | undefined;
  const committed = $derived(nearestTick(values, value));
  const index = $derived(live ?? pending ?? committed);
  const angle = $derived(indexToAngle(index, values.length));
  $effect(() => { if (pending !== null && committed === pending) pending = null; });
  function setIndex(next: number) {
    if (next === index) return;
    live = next; oninput(values[next]);
  }
  function commit() {
    const next = live; live = null;
    if (next === null || values[next] === value) return;
    pending = next; onchange(values[next]);
    clearTimeout(commitTimer); commitTimer = setTimeout(() => { pending = null; }, 4000);
  }
  function track(event: PointerEvent) {
    if (!element) return;
    const box = element.getBoundingClientRect();
    setIndex(angleToTick(pointerAngle(box.left + box.width / 2, box.top + box.height / 2, event.clientX, event.clientY), values.length, index));
  }
  function down(event: PointerEvent) {
    if (disabled || event.button !== 0) return;
    dragging = true; element?.setPointerCapture(event.pointerId); element?.focus(); track(event);
  }
  function move(event: PointerEvent) { if (dragging) track(event); }
  function up(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    if (element?.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
    commit();
  }
  function key(event: KeyboardEvent) {
    if (disabled) return;
    const steps: Record<string, number> = { ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1, PageUp: 2, PageDown: -2 };
    let next = index;
    if (event.key in steps) next = Math.min(values.length - 1, Math.max(0, index + steps[event.key]));
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = values.length - 1;
    else return;
    event.preventDefault(); setIndex(next);
    clearTimeout(commitTimer); commitTimer = setTimeout(commit, 450);
  }
  const ticks = $derived(values.map((_, tick) => indexToAngle(tick, values.length)));
</script>
<div class="knob" class:disabled class:dragging bind:this={element} role="slider" tabindex={disabled ? -1 : 0}
  aria-label={label} aria-valuemin={values[0]} aria-valuemax={values[values.length - 1]} aria-valuenow={values[index]} aria-valuetext={format(values[index])} aria-disabled={disabled}
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onkeydown={key} onblur={() => { if (live !== null) commit(); }}>
  <svg viewBox="0 0 120 120" aria-hidden="true">
    <path class="ring-track" d={arcPath(60, 60, 54, KNOB_START, 135)} />
    <path class="ring-lit" d={arcPath(60, 60, 54, KNOB_START, angle)} />
    {#each ticks as tick, tickIndex}
      <line class:lit={tickIndex <= index} x1={60 + 45 * Math.sin(tick * Math.PI / 180)} y1={60 - 45 * Math.cos(tick * Math.PI / 180)} x2={60 + 49 * Math.sin(tick * Math.PI / 180)} y2={60 - 49 * Math.cos(tick * Math.PI / 180)} />
    {/each}
  </svg>
  <span class="cap" style={`transform: rotate(${angle}deg)`}><i class="notch"></i></span>
</div>
<style>
  .knob { position: relative; width: 132px; height: 132px; border-radius: 50%; touch-action: none; cursor: grab; outline-offset: 4px; flex: 0 0 auto; }
  .knob.dragging { cursor: grabbing; }
  .knob.disabled { cursor: not-allowed; opacity: .55; }
  svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  .ring-track { fill: none; stroke: var(--neu-dark); stroke-width: 5; stroke-linecap: round; opacity: .55; }
  .ring-lit { fill: none; stroke: var(--cobalt); stroke-width: 5; stroke-linecap: round; filter: drop-shadow(0 0 3px color-mix(in srgb, var(--cobalt) 70%, transparent)); transition: d 200ms var(--spring); }
  .disabled .ring-lit { stroke: var(--muted); filter: none; }
  line { stroke: var(--muted); stroke-width: 1.6; stroke-linecap: round; opacity: .45; transition: stroke 160ms ease, opacity 160ms ease; }
  line.lit { stroke: var(--cobalt); opacity: 1; }
  .disabled line.lit { stroke: var(--muted); }
  .cap { position: absolute; inset: 24px; border-radius: 50%; background: var(--neu-bg); box-shadow: var(--raise); transition: transform 220ms var(--spring), box-shadow 180ms ease; }
  .knob:hover:not(.disabled) .cap { box-shadow: var(--raise-hover); }
  .knob.dragging .cap { box-shadow: var(--raise-sm), var(--inset-sm); }
  .cap::after { content: ""; position: absolute; inset: 14px; border-radius: 50%; box-shadow: var(--inset-sm); }
  .notch { position: absolute; left: 50%; top: 8px; width: 7px; height: 7px; margin-left: -3.5px; border-radius: 50%; background: var(--cobalt); box-shadow: var(--cobalt-glow); }
  .disabled .notch { background: var(--muted); box-shadow: none; }
</style>
