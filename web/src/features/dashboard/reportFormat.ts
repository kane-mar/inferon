// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Shared presentation atoms for the report pages, so they read the same way.
 * Pure formatters only — the one component lives in `ReportStatus`.
 */

// ITEM-124: a column header is a sentence a reader scans, not a code label. 13px
// sans, sentence case, no tracking; the row gains room so 15px text is not cramped.
//
// ITEM-148: no alignment here. `HEAD` used to carry `text-left`, so `HEAD_RIGHT`
// emitted both classes on one element and the winner depended on stylesheet order:
// a numeric column's header could sit left while its numbers sat right.
export const HEAD = 'px-3 py-2.5 text-xs font-medium text-muted-foreground';
export const CELL = 'px-3 py-2.5 text-sm tabular-nums';
/** A header cell, aligned to its column. Two pages had declared these twice. */
export const HEAD_LEFT = `${HEAD} text-left`;
export const HEAD_RIGHT = `${HEAD} text-right`;

// ITEM-147: the quantities have one home (`libs/format`), re-exported here so a
// report page imports its atoms from one module.
export { duration, micros, percent } from '@/libs/format';

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
