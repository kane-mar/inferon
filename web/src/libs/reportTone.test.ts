// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { KPI_CARD, KPI_SUBTLE, metricTone, toneGlyphClass, toneText } from './reportTone';

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

  it('leaves the KPI box neutral and lets the value carry the verdict', () => {
    // ITEM-107: colour is a highlight on the number, never a filled card — five
    // tinted boxes is decoration and the eye stops seeing "notable".
    expect(KPI_CARD).toContain('bg-card');
    expect(KPI_CARD).not.toMatch(/bg-(emerald|amber|red)-/);
  });

  it('always greys the label', () => {
    expect(KPI_SUBTLE).toBe('text-muted-foreground');
  });

  it('gives the verdict glyph the same ink as the value', () => {
    expect(toneGlyphClass('bad')).toContain('text-destructive');
    expect(toneGlyphClass('warn')).toContain('text-amber-700');
    expect(toneGlyphClass('good')).toBe('');
    expect(toneGlyphClass(undefined)).toBe('');
  });

  it('colours only the text of a table cell, never its surface', () => {
    expect(toneText('good')).toBe('');
    expect(toneText('bad')).toContain('text-destructive');
    expect(toneText('warn')).toContain('text-amber-700');
    expect(toneText('neutral')).toBe('');
  });
});
