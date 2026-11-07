// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { money } from './money';

describe('money — the report\'s one money format (ITEM-106)', () => {
  it('prints nothing for a missing value', () => {
    expect(money(null)).toBe('—');
    expect(money(undefined)).toBe('—');
    expect(money('')).toBe('—');
    expect(money('not money')).toBe('—');
  });

  it('prints whole cents with two decimals', () => {
    expect(money('0')).toBe('$0.00');
    expect(money(0)).toBe('$0.00');
    expect(money('1.5')).toBe('$1.50');
    expect(money('1234.567')).toBe('$1,234.57');
    expect(money(12)).toBe('$12.00');
  });

  it('gives sub-cent amounts their decimals instead of rounding them away', () => {
    expect(money('0.000129')).toBe('$0.000129');
    expect(money('0.005')).toBe('$0.005');
    expect(money('0.019')).toBe('$0.02');
    expect(money('0.009')).toBe('$0.009');
    expect(money('0.000010')).toBe('$0.00001');
  });

  it('never prints trailing zeros', () => {
    expect(money('0.001200')).toBe('$0.0012');
    expect(money('0.000100')).toBe('$0.0001');
  });

  it('keeps the sign outside the symbol', () => {
    expect(money('-0.000129')).toBe('-$0.000129');
    expect(money('-42.5')).toBe('-$42.50');
  });

  it('says an amount too small to show is not zero, and keeps the sign', () => {
    expect(money('0.0000004')).toBe('<$0.000001');
    expect(money('-0.0000004')).toBe('-<$0.000001');
  });
});
