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

/** Tailwind classes per tone; `neutral` adds nothing, so the value keeps its colour. */
export const TONE_CLASS: Record<Tone, string> = {
  good: 'text-emerald-600 dark:text-emerald-400',
  warn: 'text-amber-600 dark:text-amber-400',
  bad: 'text-destructive',
  neutral: '',
};

/**
 * The class for a headline value. Whole strings, chosen by tone — never
 * concatenated, so Tailwind's scanner keeps every one of them.
 */
export const toneClass = (tone: Tone | undefined): string => {
  switch (tone) {
    case 'good':
      return 'text-2xl font-semibold tabular-nums text-emerald-600 dark:text-emerald-400';
    case 'warn':
      return 'text-2xl font-semibold tabular-nums text-amber-600 dark:text-amber-400';
    case 'bad':
      return 'text-2xl font-semibold tabular-nums text-destructive';
    default:
      return 'text-2xl font-semibold tabular-nums';
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
