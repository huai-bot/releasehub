import { describe, expect, it } from 'vitest';
import { angleToTick, arcPath, counterDigits, dragOutcome, formatInterval, hoursSince, indexToAngle, nearestTick, rollTarget, sinkOffset, COUNTER_REST } from './motion';

describe('knob geometry', () => {
  it('maps ticks over a 270° sweep and snaps angles back to ticks', () => {
    expect(indexToAngle(0, 8)).toBe(-135);
    expect(indexToAngle(7, 8)).toBe(135);
    for (let index = 0; index < 8; index++) expect(angleToTick(indexToAngle(index, 8), 8, index)).toBe(index);
    expect(angleToTick(indexToAngle(3, 8) + 10, 8, 3)).toBe(3);
  });
  it('never wraps across the bottom stop', () => {
    expect(angleToTick(170, 8, 7)).toBe(7);
    expect(angleToTick(-170, 8, 7)).toBe(7);
    expect(angleToTick(-170, 8, 0)).toBe(0);
  });
  it('finds the closest tick for off-grid saved values', () => {
    expect(nearestTick([15, 30, 60, 120], 45)).toBe(1);
    expect(nearestTick([15, 30, 60, 120], 5)).toBe(0);
    expect(nearestTick([15, 30, 60, 120], 999)).toBe(3);
  });
  it('draws no arc for an empty range', () => {
    expect(arcPath(50, 50, 40, -135, -135)).toBe('');
    expect(arcPath(50, 50, 40, -135, 135)).toContain(' 1 1 ');
  });
});

describe('rolling counter', () => {
  it('pads digits and rolls lower places further', () => {
    expect(counterDigits(7)).toEqual([0, 7]);
    expect(counterDigits(1234, 2)).toEqual([1, 2, 3, 4]);
    expect(counterDigits(-3)).toEqual([0, 0]);
    const ones = rollTarget(3, 4, 0, true) - (COUNTER_REST + 3);
    const tens = rollTarget(3, 4, 1, true) - (COUNTER_REST + 3);
    expect(ones).toBeGreaterThan(tens);
    expect(rollTarget(5, 5, 0, true)).toBe(COUNTER_REST + 5);
  });
  it('stays inside the five-cycle strip in both directions', () => {
    for (let a = 0; a < 10; a++) for (let b = 0; b < 10; b++) for (const place of [0, 1, 2]) {
      for (const up of [true, false]) { const target = rollTarget(a, b, place, up); expect(target).toBeGreaterThanOrEqual(0); expect(target).toBeLessThan(50); expect(target % 10).toBe(b); }
    }
  });
});

describe('card stack and cadence', () => {
  it('sinks heavier cards further and decides the drag outcome by threshold', () => {
    expect(sinkOffset(3, 1)).toBeGreaterThan(sinkOffset(1, 1));
    expect(sinkOffset(2, 0)).toBe(0);
    expect(sinkOffset(2, 5)).toBe(sinkOffset(2, 1));
    expect(dragOutcome(109, 110)).toBe('return');
    expect(dragOutcome(110, 110)).toBe('open');
  });
  it('formats intervals and ages', () => {
    expect(formatInterval(0)).toBe('仅手动');
    expect(formatInterval(30)).toBe('30 分钟');
    expect(formatInterval(180)).toBe('3 小时');
    expect(formatInterval(1440)).toBe('1 天');
    expect(hoursSince(null, 0)).toBeNull();
    expect(hoursSince('2026-01-01T00:00:00Z', Date.parse('2026-01-01T06:00:00Z'))).toBe(6);
  });
});
