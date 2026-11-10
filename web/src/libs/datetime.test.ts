// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { dateOnly, dateTime } from './datetime';

describe('dates print as dates, not as sliced strings', () => {
  it('formats a timestamp for a reader', () => {
    expect(dateTime('2026-11-10T09:48:12Z')).toBe('10 Nov 2026, 09:48 UTC');
  });

  it('formats a day', () => {
    expect(dateOnly('2026-11-10T09:48:12Z')).toBe('10 Nov 2026');
  });

  it('is unambiguous where a slice would not be', () => {
    // The bug this replaces: `2026-11-10 09:48` reads as September 11th to an
    // American reader. The month is spelled, so it cannot.
    expect(dateTime('2026-11-10T09:48:12Z')).toContain('Nov');
    expect(dateTime('2026-11-10T09:48:12Z')).not.toContain('2026-11-10');
  });

  it('never invents a date for a missing or unparseable value', () => {
    expect(dateTime(null)).toBe('—');
    expect(dateOnly(undefined)).toBe('—');
    expect(dateTime('not a date')).toBe('not a date');
  });
});
