import type { Datasets, PayloadsReport, Quality, RuntimeReport, Summary } from './InferonApi';
import { describe, expect, it } from 'vitest';
import { attention, collectOverview } from './dashboardOverview';

const summary = (overrides: Partial<Summary> = {}): Summary => ({
  scope: 'tenant',
  tenant_id: 't1',
  window_days: 30,
  offset_days: 0,
  requests: 100,
  errors: 5,
  total_cost_usd: '1.500000',
  avg_cost_usd: '0.015000',
  total_prompt_tokens: 900,
  total_completion_tokens: 100,
  cached_prompt_tokens: 0,
  cache_hit_rate: null,
  cache_savings_usd: '0.020000',
  router_savings_usd: '0.000000',
  fallback_savings_usd: '0.000000',
  total_savings_usd: '0.020000',
  unreduced_cost_usd: '1.500000',
  cache_affinity_requests: 0,
  truncated: 0,
  truncated_rate: null,
  errors_by_class: { transport: 1 },
  avg_ttft_ms: null,
  avg_duration_ms: null,
  ttft_p50_ms: null,
  ttft_p95_ms: null,
  ttft_p99_ms: null,
  duration_p50_ms: null,
  duration_p95_ms: null,
  duration_p99_ms: null,
  proxy_p50_ms: null,
  proxy_p95_ms: null,
  first_trace_at: null,
  last_trace_at: null,
  ...overrides,
});

const quality = (overrides: Partial<Quality['overall']> = {}): Quality => ({
  window_days: 30,
  scope: 'tenant',
  overall: {
    requests: 100,
    ok: 92,
    empty: 2,
    refused: 1,
    errors: 5,
    truncated: 0,
    unclassified: 0,
    guard_fallback: 0,
    thumbs_up: 4,
    thumbs_down: 1,
    success_rate: 0.92,
    empty_rate: 0.02,
    refusal_rate: 0.01,
    error_rate: 0.05,
    guard_fallback_rate: 0,
    truncated_rate: 0,
    errors_by_class: { transport: 3, upstream_429: 2 },
    ...overrides,
  },
  models: [],
  prompts: [],
});

const performance = (
  overhead: Partial<RuntimeReport['overhead']> = {},
  latency: Partial<RuntimeReport['latency']> = {},
): RuntimeReport =>
  ({
    window: { days: 7, offset_days: 0 },
    contract: {
      p50_us: 1000,
      p95_us: 5000,
      min_samples: 20,
      window_hours: 24,
      samples: 40,
      observed_p50_us: 87,
      observed_p95_us: 900,
      p50_met: true,
      p95_met: true,
    },
    overhead: {
      samples: 40,
      under_1ms: 39,
      under_1ms_rate: 0.975,
      p50_us: 87,
      p95_us: 900,
      p99_us: 1200,
      ...overhead,
    },
    latency: {
      total_p50_ms: 800,
      total_p95_ms: 2100,
      total_p99_ms: 4000,
      ttft_p50_ms: 300,
      ttft_p95_ms: 900,
      proxy_share: 0.1,
      ...latency,
    },
    heatmap: { step: 'day', buckets: [], models: [] },
    by_model: [],
  });

const datasets = (clusters: Datasets['clusters'], grouping: 'hash' | 'cluster' = 'cluster'): Datasets => ({
  window: { days: 30 },
  role: 'owner',
  grouping,
  clusters,
});

const cluster = (overrides: Partial<Datasets['clusters'][number]> = {}) => ({
  prompt_hash: 'abc',
  model: 'deepseek-flash',
  requests: 10,
  qualified: 8,
  distinct_examples: 6,
  excluded: 2,
  excluded_by_reason: {},
  spend_usd: '0.500000',
  last_seen: '2026-10-31',
  ...overrides,
});

describe('collectOverview', () => {
  it('carries the spend headline and the token total', () => {
    const overview = collectOverview({ summary: summary() });

    expect(overview.costUsd).toBe('1.500000');
    expect(overview.tokens).toBe(1000);
    expect(overview.errorRatePct).toBe(5);
    expect(overview.savingsUsd).toBe('0.020000');
  });

  it('sums the audit findings into a recoverable total', () => {
    const overview = collectOverview({
      summary: summary(),
      waste: {
        over_provisioned: { requests: 3, savings_est_usd: '0.100000' },
        cache_miss: { requests: 2, savings_est_usd: '0.050000' },
      },
    });

    expect(overview.leakRequests).toBe(5);
    expect(Number(overview.recoverableUsd)).toBeCloseTo(0.15, 9);
  });

  it('picks the largest error class, falling back to the summary', () => {
    expect(collectOverview({ summary: summary(), quality: quality() }).topErrorClass)
      .toEqual({ name: 'transport', count: 3 });
    expect(collectOverview({ summary: summary() }).topErrorClass)
      .toEqual({ name: 'transport', count: 1 });
  });

  it('reports quality as a rate and the thumbs separately', () => {
    const overview = collectOverview({ summary: summary(), quality: quality() });

    expect(overview.okRate).toBe(0.92);
    expect(overview.thumbsUp).toBe(4);
    expect(overview.thumbsDown).toBe(1);
  });

  it('marks performance unavailable when the report did not load', () => {
    expect(collectOverview({ summary: summary() }).performance.state).toBe('unavailable');
  });

  it('marks performance empty when there are no samples', () => {
    const overview = collectOverview({
      summary: summary(),
      performance: performance({ samples: 0 }),
    });

    expect(overview.performance.state).toBe('empty');
  });

  it('reads the performance headline when samples exist', () => {
    const overview = collectOverview({
      summary: summary({ cache_hit_rate: 0.5 }),
      performance: performance(),
    });

    expect(overview.performance).toMatchObject({
      state: 'ok',
      value: {
        proxyP50Us: 87,
        under1msRate: 0.975,
        totalP95Ms: 2100,
        ttftP95Ms: 900,
        cacheHitRate: 0.5,
      },
    });
  });

  it('sums the dataset clusters and keeps the grouping', () => {
    const overview = collectOverview({
      summary: summary(),
      datasets: datasets([
        cluster(),
        cluster({ prompt_hash: 'def', distinct_examples: 4, excluded: 1, spend_usd: '0.250000' }),
      ]),
    });

    expect(overview.datasets).toMatchObject({
      state: 'ok',
      value: {
        examples: 10,
        clusters: 2,
        excluded: 3,
        grouping: 'cluster',
      },
    });
    expect(overview.datasets.state).toBe('ok');

    if (overview.datasets.state === 'ok') {
      expect(Number(overview.datasets.value.spendUsd)).toBeCloseTo(0.75, 9);
    }
  });

  it('marks datasets empty when no cluster qualified', () => {
    expect(
      collectOverview({ summary: summary(), datasets: datasets([]) }).datasets.state,
    ).toBe('empty');
  });
});

describe('attention — the landing page only lists what to act on', () => {
  const payloads = (findings: number, wasted: string): PayloadsReport => ({
    window: { days: 30 },
    requests: 100,
    capture: { enabled: true, retention_days: 30 },
    lint: {
      enabled: true,
      rules: [],
      preflight: { scanned: 100, findings, dismissed: 0, wasted_usd: wasted, by_kind: {} },
      batch: { checked: 100, flagged: 0, by_rule: {} },
    },
    pruning: {
      policy: null,
      rewritten: 0,
      holdout_requests: 0,
      tokens_saved_est: 0,
      usd_saved_est: '0',
      pruned_ok_rate: null,
      holdout_ok_rate: null,
      delta: null,
    },
    pii: {
      detection: true,
      redact_storage: false,
      redact_wire: false,
      scanned: 0,
      redacted_storage: 0,
      redacted_wire: 0,
      matches: 0,
      by_class: {},
    },
  });

  const healthy = summary({ requests: 100, errors: 1, ttft_p95_ms: 400 });

  it('is empty when nothing is out of line', () => {
    expect(attention({ summary: healthy })).toEqual([]);
  });

  it('puts a failure above money', () => {
    const findings = attention({
      summary: summary({ requests: 100, errors: 40, ttft_p95_ms: 400 }),
      waste: { over_provisioned: { requests: 3, savings_est_usd: '9.00' } },
    });

    expect(findings[0]).toMatchObject({ kind: 'errors', severity: 0 });
    expect(findings[1]).toMatchObject({ kind: 'waste', severity: 1 });
  });

  it('orders money by impact, not by source', () => {
    const findings = attention({
      summary: healthy,
      waste: { over_provisioned: { requests: 1, savings_est_usd: '0.01' } },
      payloads: payloads(2, '12.50'),
    });

    expect(findings.map(finding => finding.kind)).toEqual(['lint', 'waste']);
  });

  it('drops a lint finding that has no price', () => {
    const findings = attention({ summary: healthy, payloads: payloads(3, '0') });

    expect(findings).toEqual([]);
  });

  it('links every finding to the control that fixes it', () => {
    const findings = attention({
      summary: summary({ requests: 100, errors: 40, ttft_p95_ms: 9_000 }),
      waste: { over_provisioned: { requests: 1, savings_est_usd: '0.01' } },
      payloads: payloads(2, '12.50'),
    });

    expect(findings.map(finding => finding.href)).toEqual([
      '/dashboard/routing',
      '/dashboard/payloads',
      '/dashboard/routing',
      '/dashboard/routing',
    ]);
  });

  it('flags a slow tail as actionable latency', () => {
    const findings = attention({ summary: summary({ ttft_p95_ms: 9_000 }) });

    expect(findings[0]).toMatchObject({ kind: 'latency', href: '/dashboard/routing' });
  });
});
