import type { PayloadsReport, Summary } from './InferonApi';
import { describe, expect, it } from 'vitest';
import { attention } from './dashboardOverview';

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
