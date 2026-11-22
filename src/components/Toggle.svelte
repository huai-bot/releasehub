<script lang="ts">
  /** Soft-UI toggle: click, keyboard or drag the thumb; it snaps to the nearer side
   * with a spring. The lamp in the thumb lights while on. */
  let { checked, label, disabled = false, onchange }: { checked: boolean; label: string; disabled?: boolean; onchange: () => void } = $props();
  const TRAVEL = 22;
  let dragX = $state<number | null>(null);
  let startX = 0;
  let moved = false;
  let suppressClick = false;
  const position = $derived(dragX ?? (checked ? TRAVEL : 0));
  function down(event: PointerEvent) {
    if (disabled || event.button !== 0) return;
    startX = event.clientX; moved = false;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (disabled || !(event.currentTarget as HTMLElement).hasPointerCapture(event.pointerId)) return;
    const delta = event.clientX - startX;
    if (!moved && Math.abs(delta) < 3) return;
    moved = true;
    dragX = Math.min(TRAVEL, Math.max(0, (checked ? TRAVEL : 0) + delta));
  }
  function up(event: PointerEvent) {
    const element = event.currentTarget as HTMLElement;
    if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
    if (!moved) { dragX = null; return; }
    const next = (dragX ?? 0) > TRAVEL / 2;
    dragX = null; suppressClick = true;
    if (next !== checked) onchange();
  }
  function click() { if (suppressClick) { suppressClick = false; return; } if (!disabled) onchange(); }
</script>
<button type="button" class="neu-toggle" class:on={checked} class:dragging={dragX !== null} role="switch" aria-checked={checked} aria-label={label} {disabled}
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onclick={click}>
  <span class="track"><span class="thumb" style={`transform: translateX(${position}px)`}><i class="lamp"></i></span></span>
</button>
<style>
  .neu-toggle { padding: 4px; border-radius: 20px; flex: 0 0 auto; touch-action: none; background: var(--neu-bg); box-shadow: var(--raise-sm); transition: box-shadow 180ms ease; }
  .neu-toggle:hover:not(:disabled) { box-shadow: var(--raise); }
  .track { display: block; width: 52px; height: 30px; padding: 4px; border-radius: 16px; background: var(--neu-bg); box-shadow: var(--inset-sm); transition: background 260ms ease; }
  .on .track { background: color-mix(in srgb, var(--cobalt) 16%, var(--neu-bg)); }
  .thumb { display: grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; background: var(--neu-bg); box-shadow: var(--raise-sm); transition: transform 260ms var(--spring); }
  .dragging .thumb { transition: none; }
  .lamp { width: 7px; height: 7px; border-radius: 50%; background: var(--neu-dark); box-shadow: var(--inset-sm); transition: background 200ms ease, box-shadow 240ms var(--spring); }
  .on .lamp { background: var(--cobalt); box-shadow: var(--cobalt-glow); }
  .neu-toggle:disabled { opacity: .5; cursor: not-allowed; }
</style>
