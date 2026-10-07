import type { TimeseriesPoint } from './InferonApi';

/** Metrics the time-series chart can plot. */
export type ChartMetric = 'requests' | 'cost' | 'tokens' | 'errors' | 'ttft';

export const CHART_METRICS: ChartMetric[] = ['requests', 'cost', 'tokens', 'ttft', 'errors'];

/**
 * Numeric value of a daily point for a chart metric. `cost` is converted to a
 * number **only** to position it on an axis; the displayed money value always
 * comes from the string via `formatUsd`.
 */
export const chartValue = (point: TimeseriesPoint, metric: ChartMetric): number | null => {
  switch (metric) {
    case 'requests':
      return point.requests;
    case 'errors':
      return point.errors;
    case 'tokens':
      return point.prompt_tokens + point.completion_tokens;
    case 'cost':
      return Number(point.cost_usd);
    case 'ttft':
      return point.avg_ttft_ms;
  }
};

export const chartSeries = (points: TimeseriesPoint[], metric: ChartMetric): (number | null)[] =>
  points.map(point => chartValue(point, metric));

/**
 * Percent change from `previous` to `current`. `null` when there is no baseline
 * (previous is 0 or missing) — a jump from nothing has no meaningful percent.
 */
export const deltaPct = (
  current: number | null | undefined,
  previous: number | null | undefined,
): number | null => {
  if (current == null || previous == null || previous === 0) {
    return null;
  }
  return ((current - previous) / Math.abs(previous)) * 100;
};

/**
 * Percent change between two decimal **strings**. The values become numbers
 * only to form a *ratio*; the money amounts themselves are never rendered from
 * these numbers — they stay strings via `formatUsd`.
 */
export const moneyDeltaPct = (current: string, previous: string): number | null =>
  deltaPct(Number(current), Number(previous));

export type Trend = 'up' | 'down' | 'flat';

/** Direction of a delta, treating sub-half-percent movement as flat. */
export const trend = (delta: number | null): Trend => {
  if (delta == null || Math.abs(delta) < 0.5) {
    return 'flat';
  }
  return delta > 0 ? 'up' : 'down';
};

/** Errors as a percentage of requests (0..100), or `null` with no traffic. */
export const errorRatePct = (requests: number, errors: number): number | null =>
  requests <= 0 ? null : (errors / requests) * 100;

/**
 * ITEM-047 AC3: a row is a spike when the current window is at least `factor`×
 * the prior window and the prior window was non-trivial (>1 cent) — small
 * fluctuations never flag.
 */
export const spiked = (
  current: string,
  previous: string | null | undefined,
  factor = 2,
): boolean => {
  const now = Number(current);
  const before = Number(previous ?? 0);
  return before > 0.01 && now >= before * factor;
};

/** Exact cents → integer-safe decimal string (`formatUsd` adds the symbol). */
export const centsToUsd = (cents: number): string => {
  const sign = cents < 0 ? '-' : '';
  const abs = Math.abs(Math.round(cents));
  return `${sign}${Math.floor(abs / 100)}.${String(abs % 100).padStart(2, '0')}`;
};

const compact = (value: number): string => {
  const abs = Math.abs(value);
  const [scale, suffix]
    = abs >= 1_000_000_000
      ? [1_000_000_000, 'B']
      : abs >= 1_000_000
        ? [1_000_000, 'M']
        : abs >= 1_000
          ? [1_000, 'k']
          : [1, ''];
  const scaled = value / scale;
  const digits = Math.abs(scaled) >= 100 ? 0 : 1;
  return `${scaled.toFixed(digits).replace(/\.0$/, '')}${suffix}`;
};

/** Compact axis/legend number: 1234 → "1.2k", 1_250_000 → "1.3M". */
export const formatCompact = (value: number): string => compact(value);

/** Full integer with thousands separators: 12345 → "12,345". */
export const formatInt = (value: number): string => Math.round(value).toLocaleString('en-US');

/** Duration/latency in ms → "1.2s" past a second, else "820ms". */
export const formatMs = (value: number | null): string => {
  if (value == null) {
    return '—';
  }
  return value >= 1000 ? `${(value / 1000).toFixed(2)}s` : `${Math.round(value)}ms`;
};

const sum = (values: (number | null)[]): number =>
  values.reduce<number>((total, value) => total + (value ?? 0), 0);

/** Window totals derived from the daily series (matches the summary call). */
export const seriesTotals = (points: TimeseriesPoint[]) => ({
  requests: sum(chartSeries(points, 'requests')),
  errors: sum(chartSeries(points, 'errors')),
  tokens: sum(chartSeries(points, 'tokens')),
  cost: sum(chartSeries(points, 'cost')),
});

/** Whole-window average TTFT (ms), weighting each day's average by its requests. */
export const avgTtftMs = (points: TimeseriesPoint[]): number | null => {
  const requests = sum(points.map(point => point.requests));
  if (requests === 0) {
    return null;
  }
  const weighted = sum(points.map(point => (point.avg_ttft_ms ?? 0) * point.requests));
  return weighted / requests;
};
