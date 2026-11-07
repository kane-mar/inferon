// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Shared presentation atoms for the report pages, so they read the same way.
 * Pure formatters only — the one component lives in `ReportStatus`.
 */

// ITEM-124: a column header is a sentence a reader scans, not a code label. 13px
// sans, sentence case, no tracking; the row gains room so 15px text is not cramped.
export const HEAD = 'px-3 py-2.5 text-left text-xs font-medium text-muted-foreground';
export const CELL = 'px-3 py-2.5 text-sm tabular-nums';

export const us = (value: number | null): string =>
  value == null ? '—' : `${value.toLocaleString('en-US')} µs`;

export const ms = (value: number | null): string =>
  value == null ? '—' : `${Math.round(value).toLocaleString('en-US')} ms`;

/** A 0..1 share as a percent. `digits` is the rounding the caller needs. */
export const pct = (value: number | null | undefined, digits = 1): string =>
  value == null ? '—' : `${(value * 100).toFixed(digits)}%`;

/** A hash, short enough for a table; the caller puts the full value in `title`. */
export const shortHash = (hash: string, length = 10): string => hash.slice(0, length);

/** Scanned share of the window's traffic, or "—" with no traffic. */
export const coverage = (scanned: number, total: number): string =>
  total === 0 ? '—' : `${Math.round((scanned / total) * 100)}%`;

/**
 * A heatmap cell's tint. Values are always printed, so colour is a hint, not the
 * information — hence the deliberately low-contrast palette.
 */
export const heatCellClass = (
  cell: { error_rate: number | null; p95_ms: number | null } | null,
): string => {
  if (cell == null) {
    return 'text-muted-foreground/40';
  }
  const error = cell.error_rate ?? 0;
  if (error >= 0.2 || (cell.p95_ms ?? 0) >= 10_000) {
    return 'bg-destructive/15';
  }
  if (error >= 0.05 || (cell.p95_ms ?? 0) >= 3_000) {
    return 'bg-destructive/5';
  }
  return '';
};
