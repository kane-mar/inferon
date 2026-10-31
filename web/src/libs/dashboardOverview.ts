import type { Datasets, PerformanceReport, Quality, Summary } from './InferonApi';

/**
 * The single dashboard's numbers: the most important metric from each report,
 * read once and reduced here so the page stays a thin renderer.
 *
 * Every input is optional except the summary: one failing report must not blank
 * the dashboard (a broken performance read model should cost its card, not the
 * page).
 */
export type OverviewInput = {
  summary: Summary;
  quality?: Quality | null;
  performance?: PerformanceReport | null;
  datasets?: Datasets | null;
  /** The audit findings: `{ kind: { requests, savings_est_usd } }`. */
  waste?: Record<string, { requests: number; savings_est_usd: string }> | null;
};

/** A metric that is only meaningful once its report loaded. */
export type Maybe<T> = { state: 'ok'; value: T } | { state: 'unavailable' } | { state: 'empty' };

export type Overview = {
  requests: number;
  tokens: number;
  costUsd: string;
  avgCostUsd: string;
  savingsUsd: string;
  cacheSavingsUsd: string;
  errorRatePct: number | null;
  /** Quality: share of requests that ended `ok`. */
  okRate: number | null;
  thumbsUp: number;
  thumbsDown: number;
  topErrorClass: { name: string; count: number } | null;
  recoverableUsd: string;
  leakRequests: number;
  /** Performance: the proxy's own p50 in µs and the share under 1 ms. */
  performance: Maybe<{
    proxyP50Us: number | null;
    under1msRate: number | null;
    totalP95Ms: number | null;
    ttftP95Ms: number | null;
    cacheHitRate: number | null;
  }>;
  /** Datasets: the asset the traffic produced. */
  datasets: Maybe<{
    examples: number;
    clusters: number;
    spendUsd: string;
    excluded: number;
    grouping: 'hash' | 'cluster';
  }>;
};

const money = (value: number): string => value.toFixed(6);

/** Sum a decimal-string column without float drift beyond display precision. */
const sumStrings = (values: string[]): string =>
  money(values.reduce((total, value) => total + Number(value || 0), 0));

const topErrorClass = (
  errorsByClass: Record<string, number> | undefined,
): { name: string; count: number } | null => {
  const entries = Object.entries(errorsByClass ?? {}).sort((a, b) => b[1] - a[1]);
  const [first] = entries;
  return first ? { name: first[0], count: first[1] } : null;
};

export const collectOverview = (input: OverviewInput): Overview => {
  const { summary, quality, performance, datasets, waste } = input;
  const overall = quality?.overall;

  const leakRequests = Object.values(waste ?? {}).reduce(
    (total, row) => total + row.requests,
    0,
  );
  const recoverableUsd = sumStrings(
    Object.values(waste ?? {}).map(row => row.savings_est_usd),
  );

  return {
    requests: summary.requests,
    tokens: summary.total_prompt_tokens + summary.total_completion_tokens,
    costUsd: summary.total_cost_usd,
    avgCostUsd: summary.avg_cost_usd,
    savingsUsd: summary.total_savings_usd,
    cacheSavingsUsd: summary.cache_savings_usd,
    errorRatePct:
      summary.requests > 0 ? (summary.errors / summary.requests) * 100 : null,
    okRate: overall?.success_rate ?? null,
    thumbsUp: overall?.thumbs_up ?? 0,
    thumbsDown: overall?.thumbs_down ?? 0,
    topErrorClass: topErrorClass(overall?.errors_by_class ?? summary.errors_by_class),
    recoverableUsd,
    leakRequests,
    performance: !performance
      ? { state: 'unavailable' }
      : performance.overhead.samples === 0
        ? { state: 'empty' }
        : {
            state: 'ok',
            value: {
              proxyP50Us: performance.overhead.p50_us,
              under1msRate: performance.overhead.under_1ms_rate,
              totalP95Ms: performance.latency.total_p95_ms,
              ttftP95Ms: performance.latency.ttft_p95_ms,
              // The prompt cache is the tenant-meaningful one; the hot-path
              // counters in `performance.cache` are operator information.
              cacheHitRate: summary.cache_hit_rate,
            },
          },
    datasets: !datasets
      ? { state: 'unavailable' }
      : datasets.clusters.length === 0
        ? { state: 'empty' }
        : {
            state: 'ok',
            value: {
              examples: datasets.clusters.reduce(
                (total, cluster) => total + cluster.distinct_examples,
                0,
              ),
              clusters: datasets.clusters.length,
              spendUsd: sumStrings(datasets.clusters.map(cluster => cluster.spend_usd)),
              excluded: datasets.clusters.reduce(
                (total, cluster) => total + cluster.excluded,
                0,
              ),
              grouping: datasets.grouping,
            },
          },
  };
};
