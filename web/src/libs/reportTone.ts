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
  /** Between `good` and `warn` it is amber; beyond it, red. */
  warn: number;
  direction: 'lower' | 'higher';
};

/**
 * A whole KPI box, tinted by its verdict: background, border and a contrasting
 * foreground, so the number is readable *because* of the colour rather than
 * despite it. Whole strings — never concatenated in a `className` — so
 * Tailwind's scanner keeps every one.
 */
export const kpiCardClass = (tone: Tone | undefined): string => {
  switch (tone) {
    case 'good':
      return `rounded-lg border border-emerald-200 bg-emerald-50 p-4 text-emerald-900 shadow-sm
        dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-50`;
    case 'warn':
      return `rounded-lg border border-amber-200 bg-amber-50 p-4 text-amber-900 shadow-sm
        dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-50`;
    case 'bad':
      return `rounded-lg border border-red-200 bg-red-50 p-4 text-red-900 shadow-sm
        dark:border-red-900 dark:bg-red-950/40 dark:text-red-50`;
    default:
      return 'rounded-lg border bg-card p-4 text-card-foreground shadow-sm';
  }
};

/**
 * The label and hint inside a box. On a tinted box they inherit the foreground
 * and only soften; on a plain box they stay muted, as everywhere else.
 */
export const kpiSubtleClass = (tone: Tone | undefined): string =>
  tone == null || tone === 'neutral' ? 'text-muted-foreground' : 'opacity-75';

/**
 * The delta beside the value: green/red where colour is free (a plain box), and
 * plain but softened on a tinted one, where the box already carries the verdict.
 */
export const kpiDeltaClass = (tone: Tone | undefined, improved: boolean): string => {
  if (tone != null && tone !== 'neutral') {
    return 'text-xs font-medium opacity-75';
  }
  return improved
    ? 'text-xs font-medium text-emerald-600 dark:text-emerald-400'
    : 'text-xs font-medium text-destructive';
};

/** Table cells coloured the same way: tinted background, contrasting text. */
export const TONE_CELL: Record<Tone, string> = {
  good: 'bg-emerald-50 text-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-50',
  warn: 'bg-amber-50 text-amber-900 dark:bg-amber-950/40 dark:text-amber-50',
  bad: 'bg-red-50 text-red-900 dark:bg-red-950/40 dark:text-red-50',
  neutral: '',
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
