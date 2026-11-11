// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-147: one formatter per quantity, and one place to change how a number reads.
 *
 * The dashboard had two duration formatters printing the same value differently
 * (`1,500 ms` on the runtime page, `1.50s` on the overview), plus a third in the
 * chart axis. Every function here is pure, so the tests are the specification.
 *
 * A missing value prints an em dash: "no data" is not zero, and a table of zeroes
 * reads as a measured result.
 */

const NOTHING = '—';

/** Latency in ms: milliseconds below a second, seconds above it. */
export const duration = (value: number | null | undefined): string => {
  if (value == null) {
    return NOTHING;
  }
  if (value < 1000) {
    return `${Math.round(value)} ms`;
  }
  return `${(value / 1000).toFixed(2)} s`;
};

/** The gateway's own overhead, in microseconds: the unit its claim is made in. */
export const micros = (value: number | null | undefined): string =>
  value == null ? NOTHING : `${Math.round(value).toLocaleString('en-US')} µs`;

/** A 0..1 share as a percentage. */
export const percent = (value: number | null | undefined, digits = 1): string =>
  value == null ? NOTHING : `${(value * 100).toFixed(digits)}%`;

/** A count, grouped. */
export const integer = (value: number): string => Math.round(value).toLocaleString('en-US');
