import { describe, expect, it } from 'vitest';
import { metricTone, TONE_CLASS } from './reportTone';

describe('metricTone — a value is green when it is good, red when it is not', () => {
  it('treats a lower number as better when the metric is latency', () => {
    const p50 = { good: 1000, warn: 2000, direction: 'lower' } as const;

    expect(metricTone(500, p50)).toBe('good');
    expect(metricTone(1000, p50)).toBe('good'); // the threshold itself is good
    expect(metricTone(1500, p50)).toBe('warn');
    expect(metricTone(2000, p50)).toBe('warn');
    expect(metricTone(2500, p50)).toBe('bad');
  });

  it('treats a higher number as better when the metric is a rate', () => {
    const okRate = { good: 0.95, warn: 0.9, direction: 'higher' } as const;

    expect(metricTone(0.99, okRate)).toBe('good');
    expect(metricTone(0.95, okRate)).toBe('good');
    expect(metricTone(0.92, okRate)).toBe('warn');
    expect(metricTone(0.9, okRate)).toBe('warn');
    expect(metricTone(0.7, okRate)).toBe('bad');
  });

  it('has no opinion without a value', () => {
    const p50 = { good: 1000, warn: 2000, direction: 'lower' } as const;

    expect(metricTone(null, p50)).toBe('neutral');
    expect(metricTone(undefined, p50)).toBe('neutral');
  });

  it('maps every tone to a class, and neutral to none', () => {
    expect(TONE_CLASS.good).toContain('emerald');
    expect(TONE_CLASS.bad).toContain('destructive');
    expect(TONE_CLASS.warn).toContain('amber');
    expect(TONE_CLASS.neutral).toBe('');
  });
});
