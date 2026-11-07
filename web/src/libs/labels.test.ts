// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import en from '@/locales/en.json';
import fr from '@/locales/fr.json';
import {
  ERROR_CLASSES,
  GUARD_OUTCOMES,
  errorClassKeys,
  guardOutcomeKeys,
  isErrorClass,
  isGuardOutcome,
} from './labels';

/**
 * ITEM-125: the screen explains itself. A code the backend stores must arrive as a
 * phrase plus a line of meaning in both locales — otherwise the reader is back to
 * reading the guides to understand their own dashboard.
 */
describe('error class labels', () => {
  it.each(ERROR_CLASSES)('has a phrase and a help line in en and fr: %s', (code) => {
    expect(en.ErrorClass[code]).toBeTruthy();
    expect(en.ErrorClass[`${code}_help`]).toBeTruthy();
    expect((fr.ErrorClass as Record<string, string>)[code]).toBeTruthy();
    expect((fr.ErrorClass as Record<string, string>)[`${code}_help`]).toBeTruthy();
  });

  it('maps a known code to its two keys', () => {
    expect(errorClassKeys('upstream_429')).toEqual({
      label: 'upstream_429',
      help: 'upstream_429_help',
    });
  });

  it('keeps an unknown code readable instead of blanking the cell', () => {
    expect(isErrorClass('something_new')).toBe(false);
    expect(errorClassKeys('something_new')).toBeNull();
  });
});

describe('guard outcome labels', () => {
  it.each(GUARD_OUTCOMES)('has a phrase and a help line in en and fr: %s', (code) => {
    expect(en.Outcome[code]).toBeTruthy();
    expect(en.Outcome[`${code}_help`]).toBeTruthy();
    expect((fr.Outcome as Record<string, string>)[code]).toBeTruthy();
    expect((fr.Outcome as Record<string, string>)[`${code}_help`]).toBeTruthy();
  });

  it('rejects a value the backend does not write', () => {
    expect(isGuardOutcome('flushed')).toBe(true);
    expect(guardOutcomeKeys('exploded')).toBeNull();
  });
});
