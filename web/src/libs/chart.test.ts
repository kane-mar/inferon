// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { barSlots, monotonePath, stackHeights, ticks } from './chart';

/**
 * ITEM-152: the geometry behind the chart style. Three rules make the difference
 * between a chart that reads and one that looks busy, and all three are arithmetic:
 * three gridlines rather than five, a monotone curve that cannot dip below zero,
 * and bars that leave visible space between them.
 */
describe('ticks', () => {
  it('gives exactly three: zero, the midpoint, the maximum', () => {
    expect(ticks(100)).toEqual([0, 50, 100]);
  });

  it('rounds the top to something a reader can repeat', () => {
    // A max of 13,207 should not produce a label of 13,207.
    expect(ticks(13207)).toEqual([0, 7000, 14000]);
    // Rounded up so the tallest bar cannot cross the top gridline.
    expect(ticks(100.2)).toEqual([0, 55, 110]);
    expect(ticks(0.74)).toEqual([0, 0.37, 0.74]);
  });

  it('has an answer for no traffic at all', () => {
    expect(ticks(0)).toEqual([0, 0.5, 1]);
  });
});

describe('monotonePath', () => {
  it('never dips below the baseline between two points', () => {
    // The overshoot a Catmull-Rom spline produces; a monotone cubic cannot.
    const path = monotonePath([0, 0, 10, 0, 0], 0, 100);

    const ys = [...path.matchAll(/[-\d.]+\s([-\d.]+)/g)].map(m => Number(m[1]));

    expect(Math.min(...ys)).toBeGreaterThanOrEqual(0);
  });

  it('passes through every point it is given', () => {
    const path = monotonePath([5, 10, 7], 0, 100);

    // three points, one move command
    expect(path.startsWith('M')).toBe(true);
    expect((path.match(/C/g) ?? []).length).toBe(2);
  });

  it('paints nothing for an empty series', () => {
    expect(monotonePath([], 0, 100)).toBe('');
  });
});

describe('barSlots', () => {
  it('leaves a third of each slot empty', () => {
    const { width, pitch, gap } = barSlots(30, 300);

    expect(pitch).toBe(10);
    expect(width / pitch).toBeGreaterThanOrEqual(0.6);
    expect(width / pitch).toBeLessThanOrEqual(0.65);
    expect(gap).toBeCloseTo(pitch - width, 5);
  });

  it('keeps a bar visible when there are more slots than pixels', () => {
    const { width } = barSlots(90, 180);

    expect(width).toBeGreaterThanOrEqual(1);
  });
});

describe('stackHeights', () => {
  it('splits the column between the two segments', () => {
    expect(stackHeights([30, 10], 40, 200)).toEqual([150, 50]);
  });

  it('keeps a near-zero day visible as a sliver', () => {
    const [bottom, top] = stackHeights([0.001, 0], 40, 200);

    expect(bottom).toBeGreaterThanOrEqual(1.5);
    expect(top).toBe(0);
  });

  it('gives nothing to a genuinely empty day', () => {
    expect(stackHeights([0, 0], 40, 200)).toEqual([0, 0]);
  });
});
