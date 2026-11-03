import { money } from './money';

/**
 * Shared presentation atoms for the report pages, so they read the same way.
 * Pure formatters only — the one component lives in `ReportStatus`.
 */

export const HEAD = 'px-3 py-2 text-left font-mono text-xs uppercase text-muted-foreground';
export const CELL = 'px-3 py-2 text-sm tabular-nums';

export const us = (value: number | null): string =>
  value == null ? '—' : `${value.toLocaleString('en-US')} µs`;

export const ms = (value: number | null): string =>
  value == null ? '—' : `${Math.round(value).toLocaleString('en-US')} ms`;

export const rate = (value: number | null): string =>
  value == null ? '—' : `${(value * 100).toFixed(1)}%`;

export const integer = (value: number): string => value.toLocaleString('en-US');

/** A cost, at the precision that matters (ITEM-106). */
export const usd = (value: string | null): string => money(value);

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
