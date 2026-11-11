// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { shortHash } from './reportFormat';

describe('shortHash — a hash shown short, with the full value on hover', () => {
  it('keeps ten characters by default', () => {
    expect(shortHash('abcdef0123456789')).toBe('abcdef0123');
  });

  it('honours the caller length', () => {
    expect(shortHash('abcdef0123456789', 12)).toBe('abcdef012345');
  });
});
