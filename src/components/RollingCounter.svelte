<script lang="ts">
  /** Mechanical odometer in a recessed window. Each place rolls on its own;
   * lower places spin further in the same time (so faster) and overshoot slightly. */
  import { onDestroy, untrack } from 'svelte';
  import { counterDigits, prefersReducedMotion, rollTarget, COUNTER_REST } from '../lib/motion';
  let { value, label, minDigits = 2, size = 'md', tone = 'default' }: { value: number; label: string; minDigits?: number; size?: 'sm' | 'md' | 'lg'; tone?: 'default' | 'key' | 'alert' } = $props();
  const STRIP = Array.from({ length: 50 }, (_, index) => index % 10);
  const initial = untrack(() => ({ digits: counterDigits(value, minDigits), value: Math.max(0, Math.floor(value)) }));
  let digits = $state<number[]>(initial.digits);
  let positions = $state<number[]>(initial.digits.map((digit) => COUNTER_REST + digit));
  let animated = $state(false);
  let previous = initial.value;
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  function roll(next: number, width0: number) {
    if (next === previous) return;
    const increasing = next > previous;
    const width = Math.max(counterDigits(next, width0).length, counterDigits(previous, width0).length);
    const oldDigits = counterDigits(previous, width);
    const newDigits = counterDigits(next, width);
    previous = next;
    const reduced = prefersReducedMotion();
    digits = newDigits;
    if (positions.length !== width) positions = oldDigits.map((digit) => COUNTER_REST + digit);
    animated = !reduced;
    positions = newDigits.map((digit, index) => reduced ? COUNTER_REST + digit : rollTarget(oldDigits[index], digit, width - 1 - index, increasing));
    clearTimeout(settleTimer);
    // Re-centre silently once the roll (<=300 ms) and its overshoot have settled.
    settleTimer = setTimeout(() => { animated = false; positions = newDigits.map((digit) => COUNTER_REST + digit); }, reduced ? 0 : 340);
  }
  $effect(() => {
    const next = Math.max(0, Math.floor(Number.isFinite(value) ? value : 0));
    const width = minDigits;
    untrack(() => roll(next, width));
  });
  onDestroy(() => clearTimeout(settleTimer));
  const leading = $derived(digits.findIndex((digit) => digit !== 0));
</script>
<span class={`counter ${size} ${tone}`} role="img" aria-label={`${label} ${value}`}>
  {#each positions as position, index (digits.length - index)}
    <span class="window" class:dim={leading === -1 ? index < digits.length - 1 : index < leading}>
      <span class="strip" class:animated style={`transform: translateY(${-position}em); transition-duration: ${300 - (digits.length - 1 - index) * 40}ms`}>
        {#each STRIP as digit, cell (cell)}<span>{digit}</span>{/each}
      </span>
    </span>
  {/each}
</span>
<style>
  .counter { display: inline-flex; gap: 3px; padding: 5px 7px; border-radius: 12px; background: var(--neu-bg); box-shadow: var(--inset-sm); font-family: var(--font-num); font-weight: 650; color: var(--heading); line-height: 1; font-variant-numeric: tabular-nums; vertical-align: middle; }
  .counter.sm { font-size: 15px; border-radius: 10px; padding: 4px 6px; }
  .counter.md { font-size: 22px; }
  .counter.lg { font-size: 34px; border-radius: 16px; padding: 7px 10px; gap: 4px; }
  .counter.key { color: var(--cobalt); }
  .counter.alert { color: var(--vermilion); }
  .window { position: relative; display: block; width: .64em; height: 1em; overflow: hidden; border-radius: 4px;
    -webkit-mask-image: linear-gradient(transparent, #000 22%, #000 78%, transparent); mask-image: linear-gradient(transparent, #000 22%, #000 78%, transparent); }
  .window.dim { opacity: .32; }
  .strip { display: flex; flex-direction: column; will-change: transform; }
  .strip.animated { transition-property: transform; transition-timing-function: cubic-bezier(.3, 1.45, .6, 1); }
  .strip span { display: block; height: 1em; text-align: center; }
</style>
