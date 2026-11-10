// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Report colours: a value is green when it is healthy, red when it is not.
 *
 * The thresholds live here, in one table per metric, so a report cannot drift
 * from another one's idea of "poor". `direction` is the only thing that differs
 * between a latency (lower is better) and a rate (higher is better).
 */

export type Tone = 'good' | 'warn' | 'bad' | 'neutral';

export type MetricSpec = {
  /** At or beyond this value the number is green. */
  good: number;
  /** Between `good` and `warn` it is the warning ink; beyond it, the destructive one. */
  warn: number;
  direction: 'lower' | 'higher';
};

/**
 * The KPI box is always the neutral surface. Colour is a highlight on the value
 * (and the icon beside it), never a filled card: five coloured boxes is
 * decoration, and the eye stops telling "notable" from "normal".
 */
export const KPI_CARD = 'rounded-lg border bg-card p-5 text-card-foreground';

/**
 * The value's colour: a warning or a failure stands out; a healthy number is the
 * normal foreground, because "good" is the expected state and does not need ink.
 */
export const kpiValueClass = (tone: Tone | undefined): string => {
  switch (tone) {
    case 'bad':
      return 'text-[1.75rem]/none font-semibold tracking-tight tabular-nums text-destructive';
    case 'warn':
      return 'text-[1.75rem]/none font-semibold tracking-tight tabular-nums text-warning';
    default:
      return 'text-[1.75rem]/none font-semibold tracking-tight tabular-nums';
  }
};

/** The icon that repeats the verdict, so a reader who cannot see colour still can. */
export const toneGlyph = (tone: Tone | undefined): 'warning' | 'ok' | undefined => {
  switch (tone) {
    case 'bad':
    case 'warn':
      return 'warning';
    case 'good':
      return 'ok';
    default:
      return undefined;
  }
};

/** The label and hint inside a KPI box: muted, always. */
export const KPI_SUBTLE = 'text-muted-foreground';

/** The verdict glyph beside the value: the same ink, never a separate palette. */
export const toneGlyphClass = (tone: Tone | undefined): string => {
  switch (tone) {
    case 'bad':
      return 'text-destructive';
    case 'warn':
      return 'text-warning';
    default:
      return '';
  }
};

/**
 * The delta beside the value: green/red where colour is free (a plain box), and
 * plain but softened on a tinted one, where the box already carries the verdict.
 */
export const kpiDeltaClass = (tone: Tone | undefined, improved: boolean): string => {
  if (tone != null && tone !== 'neutral') {
    return 'text-xs font-medium opacity-75';
  }
  return improved
    ? 'text-xs font-medium text-success'
    : 'text-xs font-medium text-destructive';
};

/**
 * Table cells colour the number, not the cell: healthy stays the normal
 * foreground and only a warning or a failure takes ink (ITEM-107).
 */
export const toneText = (tone: Tone | undefined): string => {
  switch (tone) {
    case 'bad':
      return 'text-destructive font-medium';
    case 'warn':
      return 'text-warning font-medium';
    default:
      return '';
  }
};

/** The tone of a measured value, or `neutral` when there is no measurement. */
export const metricTone = (value: number | null | undefined, spec: MetricSpec): Tone => {
  if (value == null || Number.isNaN(value)) {
    return 'neutral';
  }
  const isGood = spec.direction === 'higher' ? value >= spec.good : value <= spec.good;
  if (isGood) {
    return 'good';
  }
  const isWarn = spec.direction === 'higher' ? value >= spec.warn : value <= spec.warn;
  return isWarn ? 'warn' : 'bad';
};

/** The thresholds the reports use. One table, so "poor" means the same everywhere. */
export const THRESHOLDS = {
  /** Proxy's own overhead p50, µs — the claim is sub-millisecond. */
  proxyP50Us: { good: 1_000, warn: 2_000, direction: 'lower' },
  /** Proxy's own overhead p95, µs — the guard. */
  proxyP95Us: { good: 5_000, warn: 10_000, direction: 'lower' },
  /** Share of requests whose proxy overhead is under 1 ms. */
  under1msRate: { good: 0.95, warn: 0.8, direction: 'higher' },
  /** End-to-end p95, ms (includes the model). */
  totalP95Ms: { good: 3_000, warn: 8_000, direction: 'lower' },
  /** Time to first token p95, ms. */
  ttftP95Ms: { good: 1_000, warn: 3_000, direction: 'lower' },
  /** Completed without error or refusal. */
  okRate: { good: 0.95, warn: 0.9, direction: 'higher' },
  /** Failed requests. */
  errorRate: { good: 0.01, warn: 0.05, direction: 'lower' },
  /** Responses cut off. */
  truncatedRate: { good: 0.01, warn: 0.05, direction: 'lower' },
  /** Prompts served from the provider cache. */
  cacheHitRate: { good: 0.5, warn: 0.2, direction: 'higher' },
  /** Share of the window's requests the lint actually scanned. */
  lintCoverage: { good: 0.99, warn: 0.9, direction: 'higher' },
} as const satisfies Record<string, MetricSpec>;
