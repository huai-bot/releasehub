/** Pure geometry and timing helpers for the soft-UI controls. No DOM state. */
export const KNOB_START = -135;
export const KNOB_SWEEP = 270;

export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}
export function clamp(value: number, minimum: number, maximum: number): number { return Math.min(maximum, Math.max(minimum, value)); }
/** Index of the tick closest to an arbitrary value (values must be ascending). */
export function nearestTick(values: readonly number[], value: number): number {
  if (!values.length) return 0;
  let best = 0;
  for (let index = 1; index < values.length; index++) if (Math.abs(values[index] - value) < Math.abs(values[best] - value)) best = index;
  return best;
}
/** Tick index -> knob angle (0° = up, clockwise), spread over a 270° sweep. */
export function indexToAngle(index: number, count: number): number {
  return count <= 1 ? KNOB_START : KNOB_START + (clamp(index, 0, count - 1) / (count - 1)) * KNOB_SWEEP;
}
/** Pointer position -> angle around a centre (0° = up, clockwise, range -180..180). */
export function pointerAngle(cx: number, cy: number, x: number, y: number): number {
  return (Math.atan2(x - cx, cy - y) * 180) / Math.PI;
}
/** Angle -> snapped tick. Angles in the dead zone at the bottom stick to the end the
 * pointer came from, so dragging past the stop never wraps max to min. */
export function angleToTick(angle: number, count: number, previous: number): number {
  const end = KNOB_START + KNOB_SWEEP;
  let bounded = angle;
  if (angle > end || angle < KNOB_START) bounded = previous >= (count - 1) / 2 ? end : KNOB_START;
  return clamp(Math.round(((bounded - KNOB_START) / KNOB_SWEEP) * (count - 1)), 0, count - 1);
}
/** SVG arc path between two knob angles on a circle. */
export function arcPath(cx: number, cy: number, radius: number, from: number, to: number): string {
  const point = (degrees: number) => {
    const radians = (degrees * Math.PI) / 180;
    return `${(cx + radius * Math.sin(radians)).toFixed(2)} ${(cy - radius * Math.cos(radians)).toFixed(2)}`;
  };
  const sweep = Math.max(0, to - from);
  if (sweep < 0.01) return '';
  return `M ${point(from)} A ${radius} ${radius} 0 ${sweep > 180 ? 1 : 0} 1 ${point(to)}`;
}

/** Digits of a non-negative integer, left-padded to a minimum width. */
export function counterDigits(value: number, minDigits = 2): number[] {
  const safe = Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
  return [...String(safe).padStart(minDigits, '0')].map(Number);
}
/** Strip positions for a rolling digit column (strip holds 0–9 five times).
 * Rest position is the middle cycle (20 + digit). Changed lower places travel an
 * extra cycle so they visibly spin faster than higher places. */
export const COUNTER_REST = 20;
export function rollTarget(previousDigit: number, nextDigit: number, place: number, increasing: boolean): number {
  if (previousDigit === nextDigit) return COUNTER_REST + nextDigit;
  const spins = place === 0 ? 1 : 0;
  if (increasing) return COUNTER_REST + previousDigit + ((nextDigit - previousDigit + 10) % 10) + spins * 10;
  return COUNTER_REST + previousDigit - ((previousDigit - nextDigit + 10) % 10) - spins * 10;
}

/** Rear cards sink by weight: deeper cards travel further, capped by drag progress. */
export function sinkOffset(depth: number, progress: number): number { return Math.round(clamp(progress, 0, 1) * (4 + depth * 5)); }
export function dragOutcome(distance: number, threshold: number): 'open' | 'return' { return distance >= threshold ? 'open' : 'return'; }

/** Cadence chart: hours since a check, or null when never checked. */
export function hoursSince(value: string | null, now: number): number | null {
  if (!value) return null;
  const time = Date.parse(value);
  return Number.isNaN(time) ? null : Math.max(0, (now - time) / 3_600_000);
}
export function formatInterval(minutes: number): string {
  if (minutes <= 0) return '仅手动';
  if (minutes < 60) return `${minutes} 分钟`;
  if (minutes < 1440) return `${Number((minutes / 60).toFixed(1))} 小时`;
  return `${Number((minutes / 1440).toFixed(1))} 天`;
}
