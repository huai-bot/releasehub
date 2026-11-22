<script lang="ts">
  /** 检查节奏: one module tying the automatic-check switch, the interval knob and a
   * live chart together. Bars are hours since each resource was last checked; the
   * dashed target line is the interval, so anything above it is overdue. */
  import { onMount } from 'svelte';
  import type { Project } from '../lib/types';
  import { formatInterval, hoursSince } from '../lib/motion';
  import Knob from './Knob.svelte';
  import Toggle from './Toggle.svelte';
  import RollingCounter from './RollingCounter.svelte';
  let { projects, interval, busy = false, onsave, onselect }: {
    projects: Project[]; interval: number; busy?: boolean;
    onsave: (minutes: number) => Promise<boolean> | boolean; onselect?: (project: Project) => void;
  } = $props();
  const TICKS = [15, 30, 60, 120, 180, 360, 720, 1440] as const;
  const MEMORY = 'releasehub.lastCheckInterval';
  let now = $state(Date.now());
  let live = $state<number | null>(null);
  let remembered = $state(60);
  const enabled = $derived(interval > 0);
  const target = $derived(live ?? (enabled ? interval : remembered));
  const targetHours = $derived(target / 60);
  const rows = $derived(projects.map((project) => ({ project, hours: hoursSince(project.checkedAt, now) })));
  const checked = $derived(rows.filter((row): row is { project: Project; hours: number } => row.hours !== null).sort((a, b) => b.hours - a.hours));
  const neverChecked = $derived(rows.length - checked.length);
  const bars = $derived(checked.slice(0, 14));
  const scale = $derived(Math.max(targetHours * 1.35, ...bars.map((bar) => bar.hours), 0.5) * 1.08);
  const overdue = $derived(checked.filter((row) => row.hours > targetHours).length + neverChecked);
  const onTime = $derived(Math.max(0, projects.length - overdue));
  onMount(() => {
    let saved = 0;
    try { saved = Number(globalThis.localStorage?.getItem(MEMORY)); } catch { /* storage unavailable */ }
    if (TICKS.includes(saved as (typeof TICKS)[number])) remembered = saved;
    if (interval > 0) remembered = interval;
    const timer = setInterval(() => { now = Date.now(); }, 60_000);
    return () => clearInterval(timer);
  });
  function remember(minutes: number) { remembered = minutes; try { globalThis.localStorage?.setItem(MEMORY, String(minutes)); } catch { /* optional */ } }
  async function toggle() {
    if (busy) return;
    if (enabled) { remember(interval); await onsave(0); }
    else await onsave(remembered);
  }
  async function change(minutes: number) {
    remember(minutes);
    await onsave(minutes);
    live = null;
  }
  const hoursLabel = (hours: number) => hours < 1 ? `${Math.round(hours * 60)} 分钟前` : hours < 48 ? `${hours.toFixed(1)} 小时前` : `${Math.round(hours / 24)} 天前`;
</script>

<section class="neu-module cadence" class:off={!enabled} aria-labelledby="cadence-title">
  <header class="neu-module-header">
    <div><h2 id="cadence-title">检查节奏</h2><p>{enabled ? `每 ${formatInterval(target)} 自动检查一次` : '自动检查已关闭，仅手动检查'}</p></div>
    <div class="neu-module-switch"><span class="neu-led" class:on={enabled}></span><Toggle checked={enabled} label="自动检查" disabled={busy} onchange={() => void toggle()} /></div>
  </header>
  <div class="cadence-body">
    <div class="cadence-control">
      <Knob values={TICKS} value={enabled ? interval : remembered} label="自动检查间隔" format={formatInterval} disabled={busy || !enabled} oninput={(minutes) => live = minutes} onchange={(minutes) => void change(minutes)} />
      <div class="cadence-readout neu-well"><small>目标间隔</small><strong class:key-data={enabled}>{formatInterval(target)}</strong></div>
    </div>
    <div class="cadence-chart">
      <div class="cadence-stats">
        <span><RollingCounter value={onTime} label="按时检查" size="sm" tone={enabled ? 'key' : 'default'} />按时</span>
        <span><RollingCounter value={overdue} label="超出目标" size="sm" tone={overdue && enabled ? 'alert' : 'default'} />超出目标</span>
      </div>
      <div class="plot" role="img" aria-label={`距上次检查的时长，目标线 ${formatInterval(target)}；${overdue} 项超出目标`}>
        <div class="target-line" style={`bottom:${Math.min(100, (targetHours / scale) * 100)}%`}><span>{formatInterval(target)}</span></div>
        {#each bars as bar (bar.project.id)}
          <button class="bar" class:over={bar.hours > targetHours} style={`height:${Math.max(3, (bar.hours / scale) * 100)}%`} title={`${bar.project.name} · ${hoursLabel(bar.hours)}`} aria-label={`${bar.project.name}，${hoursLabel(bar.hours)}检查`} onclick={() => onselect?.(bar.project)}></button>
        {:else}
          <p class="plot-empty">{projects.length ? '资源尚未检查' : '添加资源后显示检查时效'}</p>
        {/each}
      </div>
      {#if neverChecked}<p class="cadence-note">{neverChecked} 项从未检查，计入超出目标</p>{/if}
    </div>
  </div>
</section>

<style>
  .cadence-body { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 26px; align-items: stretch; }
  .cadence-control { display: flex; flex-direction: column; align-items: center; gap: 14px; }
  .cadence-readout { display: flex; flex-direction: column; align-items: center; padding: 8px 16px; min-width: 120px; }
  .cadence-readout small { color: var(--muted); font-size: 11px; }
  .cadence-readout strong { font-size: 17px; font-family: var(--font-num); }
  .cadence-chart { display: flex; flex-direction: column; min-width: 0; }
  .cadence-stats { display: flex; gap: 22px; margin-bottom: 12px; color: var(--muted); font-size: 12px; }
  .cadence-stats span { display: inline-flex; align-items: center; gap: 8px; }
  .plot { position: relative; flex: 1; min-height: 150px; display: flex; align-items: flex-end; gap: 6px; padding: 14px 14px 0; border-radius: 16px; box-shadow: var(--inset); overflow: hidden;
    transition: filter 300ms ease, opacity 300ms ease; }
  .off .plot { filter: grayscale(1); opacity: .55; }
  .bar { flex: 1; min-width: 6px; max-width: 34px; padding: 0; border-radius: 8px 8px 3px 3px; background: linear-gradient(180deg, color-mix(in srgb, var(--cobalt) 78%, #fff), var(--cobalt));
    transition: height 260ms var(--spring), background 220ms ease; box-shadow: 0 -2px 6px color-mix(in srgb, var(--cobalt) 30%, transparent); }
  .bar.over { background: linear-gradient(180deg, color-mix(in srgb, var(--vermilion) 75%, #fff), var(--vermilion)); box-shadow: 0 -2px 6px color-mix(in srgb, var(--vermilion) 30%, transparent); }
  .bar:hover { filter: brightness(1.08); }
  .bar:focus-visible { outline-offset: 2px; }
  .target-line { position: absolute; left: 10px; right: 10px; height: 0; border-top: 2px dashed var(--cobalt); transition: bottom 220ms var(--spring); z-index: 1; pointer-events: none; }
  .target-line span { position: absolute; right: 0; top: -22px; font-size: 11px; font-family: var(--font-num); color: var(--cobalt); background: var(--neu-bg); padding: 1px 6px; border-radius: 8px; box-shadow: var(--raise-sm); }
  .plot-empty { margin: auto; padding-bottom: 50px; color: var(--muted); font-size: 12px; }
  .cadence-note { margin-top: 8px; color: var(--muted); font-size: 11px; }
  @media (max-width: 1100px) { .cadence-body { gap: 16px; } }
</style>
