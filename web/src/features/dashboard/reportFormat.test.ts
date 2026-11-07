// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { pct, shortHash } from './reportFormat';

describe('pct — one percent format for every report', () => {
  it('renders a 0..1 share with one decimal by default', () => {
    expect(pct(0.956)).toBe('95.6%');
    expect(pct(1)).toBe('100.0%');
  });

  it('rounds to the digits the caller needs', () => {
    expect(pct(0.956, 0)).toBe('96%');
    expect(pct(0.1234, 2)).toBe('12.34%');
  });

  it('renders a dash without a value', () => {
    expect(pct(null)).toBe('—');
    expect(pct(undefined)).toBe('—');
  });
});

describe('shortHash — a hash shown short, with the full value on hover', () => {
  it('keeps ten characters by default', () => {
    expect(shortHash('abcdef0123456789')).toBe('abcdef0123');
  });

  it('honours the caller length', () => {
    expect(shortHash('abcdef0123456789', 12)).toBe('abcdef012345');
  });
});
