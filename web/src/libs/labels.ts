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

/** i18n keys: `ErrorClass.<code>` is the phrase, `ErrorClass.<code>_help` the line. */
export type ErrorLabelKey = ErrorClassCode | `${ErrorClassCode}_help`;
export type OutcomeLabelKey = GuardOutcomeCode | `${GuardOutcomeCode}_help`;

export const isErrorClass = (value: string): value is ErrorClassCode =>
  (ERROR_CLASSES as readonly string[]).includes(value);

export const isGuardOutcome = (value: string): value is GuardOutcomeCode =>
  (GUARD_OUTCOMES as readonly string[]).includes(value);

/** The keys to ask the locale for, or `null` when the code is unknown. */
export const errorClassKeys = (
  value: string,
): { label: ErrorLabelKey; help: ErrorLabelKey } | null =>
  isErrorClass(value) ? { label: value, help: `${value}_help` } : null;

export const guardOutcomeKeys = (
  value: string,
): { label: OutcomeLabelKey; help: OutcomeLabelKey } | null =>
  isGuardOutcome(value) ? { label: value, help: `${value}_help` } : null;
