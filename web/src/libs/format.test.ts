// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { duration, integer, micros, percent } from './format';

/**
 * ITEM-147: one formatter per quantity.
 *
 * Before this there were two: `reportFormat.ms` printed `1,500 ms` and
 * `dashboardMetrics.formatMs` printed `1.50s` for the same value, so the same
 * metric looked different on the overview and on the runtime page.
 */
describe('duration', () => {
  it('prints milliseconds below a second', () => {
    expect(duration(0)).toBe('0 ms');
    expect(duration(820)).toBe('820 ms');
    expect(duration(999)).toBe('999 ms');
  });

  it('prints seconds from a second up, because 8,928 ms is harder to scan', () => {
    expect(duration(1000)).toBe('1.00 s');
    expect(duration(1500)).toBe('1.50 s');
    expect(duration(8928)).toBe('8.93 s');
    expect(duration(60000)).toBe('60.00 s');
  });

  it('says nothing rather than zero for a missing value', () => {
    expect(duration(null)).toBe('—');
    expect(duration(undefined)).toBe('—');
  });

  it('rounds the way a reader expects at each scale', () => {
    // 999.4 stays in ms and rounds; 999.6 crosses into the second branch.
    expect(duration(999.4)).toBe('999 ms');
    expect(duration(1000.4)).toBe('1.00 s');
  });
});

describe('micros', () => {
  it('keeps the unit the gateway claim is made in', () => {
    expect(micros(0)).toBe('0 µs');
    expect(micros(1234)).toBe('1,234 µs');
    expect(micros(23031)).toBe('23,031 µs');
  });

  it('says nothing for a missing value', () => {
    expect(micros(null)).toBe('—');
  });
});

describe('percent', () => {
  it('takes a 0..1 share and prints the share', () => {
    expect(percent(0.1234)).toBe('12.3%');
    expect(percent(1)).toBe('100.0%');
  });

  it('accepts the rounding the caller needs', () => {
    expect(percent(0.1234, 0)).toBe('12%');
    expect(percent(0.1234, 2)).toBe('12.34%');
  });

  it('says nothing for a missing value', () => {
    expect(percent(null)).toBe('—');
    expect(percent(undefined)).toBe('—');
  });
});

describe('integer', () => {
  it('groups thousands and rounds', () => {
    expect(integer(1234.6)).toBe('1,235');
    expect(integer(0)).toBe('0');
    expect(integer(1234567)).toBe('1,234,567');
  });
});
