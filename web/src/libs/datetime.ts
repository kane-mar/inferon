// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-143: dates in the reader's locale, not sliced strings.
 *
 * `value.slice(0, 16).replace('T', ' ')` prints `2026-11-10 09:48` for every
 * reader on earth: an American reads it as September 11th, and nobody gets their
 * own convention. `Intl` is available in every runtime this app ships on, so the
 * only reason to hand-roll was habit.
 *
 * Fixed to UTC because these timestamps are storage values (a job's created_at, an
 * alert's last fire) and two operators in different zones comparing a log line
 * should see one answer. The zone is printed so nobody has to guess.
 */

const DATE_TIME = new Intl.DateTimeFormat('en-GB', {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  timeZone: 'UTC',
  timeZoneName: 'short',
});

const DATE_ONLY = new Intl.DateTimeFormat('en-GB', {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
  timeZone: 'UTC',
});

/** `2026-11-10T09:48:12Z` → `10 Nov 2026, 09:48 UTC`. */
export const dateTime = (value: string | null | undefined): string => {
  if (!value) {
    return '—';
  }
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? value : DATE_TIME.format(at);
};

/** `2026-11-10T09:48:12Z` → `10 Nov 2026`. */
export const dateOnly = (value: string | null | undefined): string => {
  if (!value) {
    return '—';
  }
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? value : DATE_ONLY.format(at);
};
