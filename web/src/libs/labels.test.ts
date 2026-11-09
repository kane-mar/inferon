// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import en from '@/locales/en.json';
import fr from '@/locales/fr.json';
import {
  ERROR_CLASSES,
  errorClassKeys,
  GUARD_OUTCOMES,
  guardOutcomeKeys,
  isErrorClass,
  isGuardOutcome,
} from './labels';

/**
 * ITEM-125/138: a stored code arrives as a phrase in both locales, and the phrase
 * has to stand alone. There is no second "line of meaning" on the screen any more;
 * a term that needs one is a term that needs renaming.
 */
describe('error class labels', () => {
  it.each(ERROR_CLASSES)('has a phrase in en and fr: %s', (code) => {
    expect(en.ErrorClass[code]).toBeTruthy();
    expect((fr.ErrorClass as Record<string, string>)[code]).toBeTruthy();
  });

  it.each(ERROR_CLASSES)('keeps the phrase short enough to read at a glance: %s', (code) => {
    // ITEM-138: the interface states, it does not explain. A phrase running into a
    // clause is the start of a paragraph.
    expect(en.ErrorClass[code]?.length ?? 0).toBeLessThanOrEqual(30);
    expect((fr.ErrorClass as Record<string, string>)[code]?.length ?? 0).toBeLessThanOrEqual(36);
  });

  it('maps a known code to its key', () => {
    expect(errorClassKeys('upstream_429')).toBe('upstream_429');
  });

  it('keeps an unknown code readable instead of blanking the cell', () => {
    expect(isErrorClass('something_new')).toBe(false);
    expect(errorClassKeys('something_new')).toBeNull();
  });
});

describe('guard outcome labels', () => {
  it.each(GUARD_OUTCOMES)('has a phrase and a help line in en and fr: %s', (code) => {
    expect(en.Outcome[code]).toBeTruthy();
    expect((fr.Outcome as Record<string, string>)[code]).toBeTruthy();
  });

  it('rejects a value the backend does not write', () => {
    expect(isGuardOutcome('flushed')).toBe(true);
    expect(guardOutcomeKeys('exploded')).toBeNull();
  });
});
