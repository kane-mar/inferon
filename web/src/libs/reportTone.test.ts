import { describe, expect, it } from 'vitest';
import { kpiCardClass, kpiSubtleClass, metricTone, TONE_CELL } from './reportTone';

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

  it('tints the whole box, not just the number', () => {
    // A tinted box carries its own background, border and contrasting text, so
    // the number is readable because of the colour rather than despite it.
    for (const [tone, colour] of [
      ['good', 'emerald'],
      ['warn', 'amber'],
      ['bad', 'red'],
    ] as const) {
      const classes = kpiCardClass(tone);

      expect(classes).toContain(`bg-${colour}-50`);
      expect(classes).toContain(`border-${colour}-200`);
      expect(classes).toContain(`text-${colour}-900`);
    }
    // The plain box is unchanged: the app's card surface and foreground.

    expect(kpiCardClass(undefined)).toContain('bg-card');
    expect(kpiCardClass('neutral')).toContain('text-card-foreground');
  });

  it('softens the label on a tinted box instead of greying it out', () => {
    expect(kpiSubtleClass('good')).toBe('opacity-75');
    expect(kpiSubtleClass('bad')).toBe('opacity-75');
    expect(kpiSubtleClass('neutral')).toBe('text-muted-foreground');
    expect(kpiSubtleClass(undefined)).toBe('text-muted-foreground');
  });

  it('tints table cells the same way, leaving neutral alone', () => {
    expect(TONE_CELL.good).toContain('bg-emerald-50');
    expect(TONE_CELL.bad).toContain('bg-red-50');
    expect(TONE_CELL.warn).toContain('bg-amber-50');
    expect(TONE_CELL.neutral).toBe('');
  });
});
