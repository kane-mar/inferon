// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { kpiCaption } from './kpiCaption';

describe('kpiCaption', () => {
  it('prints the definition on its own when there is no verdict', () => {
    expect(kpiCaption({ definition: 'Measured before dispatch.' })).toEqual([
      'Measured before dispatch.',
    ]);
  });

  it('keeps the verdict above the definition when both are given', () => {
    expect(
      kpiCaption({
        hint: 'over the 1 ms claim · median prompt 288,494 tokens',
        definition: 'Measured before dispatch.',
      }),
    ).toEqual(['over the 1 ms claim · median prompt 288,494 tokens', 'Measured before dispatch.']);
  });

  it('drops empty lines rather than rendering blank space', () => {
    expect(kpiCaption({ hint: '   ', definition: undefined })).toEqual([]);
  });
});
