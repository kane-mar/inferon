// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-152: the arithmetic behind the chart style.
 *
 * Three decisions do most of the work in a quiet chart, and none of them is
 * subjective: **three** gridlines instead of five, a curve that cannot overshoot
 * the baseline, and bars that leave a third of their slot empty. Keeping them here
 * (rather than inside SVG JSX) is what makes them testable.
 */

/**
 * A tick value rounded **up** to two significant figures: 13,207 becomes 14,000.
 * Up, never down, so the tallest bar cannot poke through the top gridline; and two
 * figures, because an axis label of 13,207 is noise on a chart this quiet.
 */
const roundTick = (value: number): number => {
  if (value === 0) {
    return 0;
  }
  const step = 10 ** Math.floor(Math.log10(value)) / 10;
  return Number((Math.ceil(value / step) * step).toPrecision(12));
};

/** Exactly three axis values: zero, the midpoint, the rounded maximum. */
export const ticks = (max: number): [number, number, number] => {
  if (!(max > 0)) {
    return [0, 0.5, 1];
  }
  const top = roundTick(max);
  return [0, top / 2, top];
};

/**
 * A monotone cubic path through `values`, mapped from `0..max` to `height..0`.
 *
 * Monotone because a plain Catmull-Rom overshoots: between two zeros and a peak it
 * dips below the baseline, which in an area chart reads as negative traffic. The
 * tangent at each point is limited to the smaller neighbouring secant, so the curve
 * never passes a value it was not given.
 */
export const monotonePath = (values: number[], max: number, height: number): string => {
  if (values.length === 0) {
    return '';
  }
  const top = max > 0 ? max : 1;
  const step = values.length > 1 ? height / (values.length - 1) : 0;
  void step;
  const xs = values.map((_, index) => (values.length > 1 ? index / (values.length - 1) : 0.5));
  const ys = values.map(value => 1 - Math.min(1, Math.max(0, value / top)));

  if (values.length === 1) {
    return `M0 ${(ys[0] ?? 0).toFixed(4)}`;
  }

  // Secants between neighbouring points, then a tangent per point.
  const secants = xs.slice(1).map((x, index) => {
    const dx = x - (xs[index] ?? 0);
    const dy = (ys[index + 1] ?? 0) - (ys[index] ?? 0);
    return dx === 0 ? 0 : dy / dx;
  });
  const tangents = xs.map((_, index) => {
    const previous = secants[index - 1];
    const next = secants[index];
    if (previous === undefined) {
      return next ?? 0;
    }
    if (next === undefined) {
      return previous;
    }
    // Monotone: the tangent may not exceed either neighbouring secant. Zero where
    // the direction changes, which is what removes the overshoot at a peak.
    if (previous * next <= 0) {
      return 0;
    }
    return (Math.sign(previous) * Math.min(Math.abs(previous), Math.abs(next), Math.abs((previous + next) / 2)));
  });

  let path = `M${(xs[0] ?? 0).toFixed(4)} ${(ys[0] ?? 0).toFixed(4)}`;
  for (let index = 0; index < values.length - 1; index += 1) {
    const x0 = xs[index] ?? 0;
    const x1 = xs[index + 1] ?? 0;
    const y0 = ys[index] ?? 0;
    const y1 = ys[index + 1] ?? 0;
    const third = (x1 - x0) / 3;
    const c1x = x0 + third;
    const c1y = y0 + (tangents[index] ?? 0) * third;
    const c2x = x1 - third;
    const c2y = y1 - (tangents[index + 1] ?? 0) * third;
    path += ` C${c1x.toFixed(4)} ${c1y.toFixed(4)} ${c2x.toFixed(4)} ${c2y.toFixed(4)} ${x1.toFixed(4)} ${y1.toFixed(4)}`;
  }
  return path;
};

/** Slot geometry for `count` bars across `innerWidth`: 60–65% bar, the rest gap. */
export const barSlots = (
  count: number,
  innerWidth: number,
): { pitch: number; width: number; gap: number } => {
  const pitch = count > 0 ? innerWidth / count : innerWidth;
  const width = Math.max(1, pitch * 0.62);
  return { pitch, width, gap: pitch - width };
};

/**
 * The two stack heights for one column, in pixels, from the bottom of the plot.
 *
 * A day with any traffic keeps a 1.5px sliver so the daily cadence stays unbroken;
 * a day with none gets nothing at all, because a sliver on an empty day is a lie.
 */
export const stackHeights = (
  segments: [number, number],
  max: number,
  plotheight: number,
): [number, number] => {
  const top = max > 0 ? max : 1;
  const [bottomValue, topValue] = segments;
  const bottom = bottomValue > 0 ? Math.max(1.5, (bottomValue / top) * plotheight) : 0;
  const topPart = topValue > 0 ? Math.max(1.5, (topValue / top) * plotheight) : 0;
  return [bottom, topPart];
};
