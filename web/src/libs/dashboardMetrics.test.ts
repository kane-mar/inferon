import type { TimeseriesPoint } from './InferonApi';
import { describe, expect, it } from 'vitest';
import {
  avgTtftMs,
  chartSeries,
  chartValue,
  deltaPct,
  errorRatePct,
  formatCompact,
  formatInt,
  formatMs,
  moneyDeltaPct,
  seriesTotals,
  trend,
} from './dashboardMetrics';

const point = (overrides: Partial<TimeseriesPoint> = {}): TimeseriesPoint => ({
  date: '2026-10-05',
  requests: 10,
  errors: 1,
  cost_usd: '0.25',
  prompt_tokens: 100,
  completion_tokens: 50,
  avg_ttft_ms: 400,
  avg_duration_ms: 1200,
  ...overrides,
});

describe('chartValue', () => {
  it('reads each metric from a daily point', () => {
    expect(chartValue(point(), 'requests')).toBe(10);
    expect(chartValue(point(), 'errors')).toBe(1);
    expect(chartValue(point(), 'tokens')).toBe(150);
    expect(chartValue(point(), 'cost')).toBeCloseTo(0.25);
    expect(chartValue(point(), 'ttft')).toBe(400);
  });

  it('returns null for ttft on days with no requests', () => {
    expect(chartValue(point({ avg_ttft_ms: null }), 'ttft')).toBeNull();
  });

  it('maps a series in order', () => {
    const points = [point({ requests: 1 }), point({ requests: 2 })];

    expect(chartSeries(points, 'requests')).toEqual([1, 2]);
  });
});

describe('deltaPct / trend', () => {
  it('computes percent change', () => {
    expect(deltaPct(150, 100)).toBeCloseTo(50);
    expect(deltaPct(50, 100)).toBeCloseTo(-50);
  });

  it('has no baseline to compare against', () => {
    expect(deltaPct(10, 0)).toBeNull();
    expect(deltaPct(10, null)).toBeNull();
    expect(deltaPct(null, 10)).toBeNull();
  });

  it('compares decimal cost strings as a display-only ratio', () => {
    expect(moneyDeltaPct('0.012', '0.010')).toBeCloseTo(20);
    expect(moneyDeltaPct('0.000007', '0.000000')).toBeNull();
  });

  it('classifies direction with a flat deadband', () => {
    expect(trend(12)).toBe('up');
    expect(trend(-12)).toBe('down');
    expect(trend(0.2)).toBe('flat');
    expect(trend(null)).toBe('flat');
  });
});

describe('errorRatePct', () => {
  it('is a share of requests', () => {
    expect(errorRatePct(200, 5)).toBeCloseTo(2.5);
  });

  it('is undefined with no requests', () => {
    expect(errorRatePct(0, 0)).toBeNull();
  });
});

describe('number formatting', () => {
  it('compacts large counts', () => {
    expect(formatCompact(999)).toBe('999');
    expect(formatCompact(1234)).toBe('1.2k');
    expect(formatCompact(12_000)).toBe('12k');
    expect(formatCompact(1_250_000)).toBe('1.3M');
  });

  it('adds thousands separators', () => {
    expect(formatInt(12345)).toBe('12,345');
  });

  it('renders latency in ms or seconds', () => {
    expect(formatMs(820)).toBe('820ms');
    expect(formatMs(1250)).toBe('1.25s');
    expect(formatMs(null)).toBe('—');
  });
});

describe('seriesTotals / avgTtftMs', () => {
  it('sums the additive metrics across days', () => {
    const totals = seriesTotals([
      point({ requests: 2, errors: 0, prompt_tokens: 10, completion_tokens: 5, cost_usd: '0.1' }),
      point({ requests: 3, errors: 1, prompt_tokens: 20, completion_tokens: 5, cost_usd: '0.2' }),
    ]);

    expect(totals.requests).toBe(5);
    expect(totals.errors).toBe(1);
    expect(totals.tokens).toBe(40);
    expect(totals.cost).toBeCloseTo(0.3);
  });

  it('weights the ttft average by requests and ignores empty days', () => {
    const avg = avgTtftMs([
      point({ requests: 1, avg_ttft_ms: 100 }),
      point({ requests: 3, avg_ttft_ms: 200 }),
      point({ requests: 0, avg_ttft_ms: null }),
    ]);

    expect(avg).toBeCloseTo(175);
    expect(avgTtftMs([point({ requests: 0, avg_ttft_ms: null })])).toBeNull();
  });
});
