// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { share, split } from './segments';

/**
 * ITEM-148: six KPI cards, five of them failure rates, read as six alarms. One
 * bar showing what the requests actually were tells the same story in a glance.
 * The arithmetic is here so the widths can be trusted (and tested) rather than
 * eyeballed in a component.
 */
describe('split', () => {
  it('turns counts into percentages that sum to 100', () => {
    const parts = split([
      { key: 'ok', count: 90 },
      { key: 'empty', count: 10 },
    ]);

    expect(parts).toEqual([
      { key: 'ok', count: 90, share: 90 },
      { key: 'empty', count: 10, share: 10 },
    ]);
  });

  it('drops the empty segments, so a healthy bar has no invisible slivers', () => {
    const parts = split([
      { key: 'ok', count: 100 },
      { key: 'empty', count: 0 },
      { key: 'error', count: 0 },
    ]);

    expect(parts.map(part => part.key)).toEqual(['ok']);
  });

  it('keeps a real sliver visible rather than rounding it away', () => {
    // 1 in 1000: at 0.1% a bar segment would be a pixel. The rule is a floor of
    // 0.4%, so the reader can see that something is there.
    const parts = split([
      { key: 'ok', count: 999 },
      { key: 'error', count: 1 },
    ]);
    const error = parts.find(part => part.key === 'error');

    expect(error?.share).toBeGreaterThanOrEqual(0.4);
  });

  it('has nothing to say about no traffic', () => {
    expect(split([])).toEqual([]);
    expect(split([{ key: 'ok', count: 0 }])).toEqual([]);
  });
});

describe('share', () => {
  it('is the part over the whole, as a percentage', () => {
    expect(share(25, 100)).toBe(25);
    expect(share(1, 3)).toBeCloseTo(33.3, 1);
  });

  it('is null rather than a division by zero', () => {
    expect(share(0, 0)).toBeNull();
    expect(share(5, 0)).toBeNull();
  });
});
