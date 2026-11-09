// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-125: the screen names things in the reader's words.
 *
 * The API stores a closed set of error classes and guard outcomes. Printing those
 * strings verbatim ("upstream_429", "fallback_failed") asked the reader to learn
 * our vocabulary or open the guides — which is the design failure this file fixes.
 *
 * The set lives here, in one place, so a new class cannot be added to the backend
 * without a decision about how it reads. An unknown value still renders: it falls
 * back to the raw code rather than a blank cell, because a blank cell is worse
 * than a word the reader has to look up.
 */

/** Mirrors `ErrorClass::as_str` in `src/errors.rs`. */
export const ERROR_CLASSES = [
  'upstream_429',
  'upstream_5xx',
  'client_4xx',
  'inferon_4xx',
  'rate_limited',
  'inferon_5xx',
  'transport',
  'timeout',
  'guard',
  'client_abort',
] as const;

export type ErrorClassCode = (typeof ERROR_CLASSES)[number];

/** Mirrors the `guard_event.outcome` values the checker writes. */
export const GUARD_OUTCOMES = ['flushed', 'fallback', 'fallback_failed'] as const;

export type GuardOutcomeCode = (typeof GUARD_OUTCOMES)[number];

/**
 * ITEM-138: one phrase per code. The second "line of meaning" is gone — it was a
 * sentence on a screen. The phrase has to stand alone; anything more is a guide.
 */
export type ErrorLabelKey = ErrorClassCode;
export type OutcomeLabelKey = GuardOutcomeCode;

export const isErrorClass = (value: string): value is ErrorClassCode =>
  (ERROR_CLASSES as readonly string[]).includes(value);

export const isGuardOutcome = (value: string): value is GuardOutcomeCode =>
  (GUARD_OUTCOMES as readonly string[]).includes(value);

/** The locale key for a code, or `null` when the code is unknown. */
export const errorClassKeys = (value: string): ErrorLabelKey | null =>
  isErrorClass(value) ? value : null;

export const guardOutcomeKeys = (value: string): OutcomeLabelKey | null =>
  isGuardOutcome(value) ? value : null;
