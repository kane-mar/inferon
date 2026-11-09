// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { driftCellClass, driftMark, driftMeaning } from './bandDrift';

describe('band drift presentation', () => {
  it('tints only a comparable regression', () => {
    expect(driftCellClass('regression')).toContain('text-destructive');
    expect(driftCellClass('watch')).toContain('text-warning');
  });

  it('never tints a payload move as a fault', () => {
    // The whole point: a bigger prompt looks different from a slower gateway.
    const moved = driftCellClass('moved');

    expect(moved).not.toContain('destructive');
    expect(moved).not.toContain('warning');
    expect(moved).toContain('muted-foreground');
  });

  it('leaves a calm bucket unmarked', () => {
    expect(driftCellClass('calm')).toBe('');
    expect(driftCellClass(null)).toBe('');
    expect(driftMark('calm')).toBe('');
  });

  it('gives every verdict a glyph, so colour is never the only signal', () => {
    expect(driftMark('regression')).toBe('▲');
    expect(driftMark('watch')).toBe('△');
    expect(driftMark('moved')).toBe('~');
  });

  it('treats a missing verdict as calm for the legend', () => {
    expect(driftMeaning(null)).toBe('calm');
    expect(driftMeaning('moved')).toBe('moved');
  });
});
