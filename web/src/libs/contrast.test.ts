// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * The palette's contract, measured rather than trusted.
 *
 * A design system fails quietly: one hand-picked "nicer grey" at 4.4:1 and the
 * whole dashboard is unreadable for somebody, with nothing in the build to say
 * so. These tests read `styles/global.css` and hold it to the rules the
 * legibility pass set (ITEM-124) plus the colour discipline this pass added
 * (ITEM-134):
 *
 *   - body ink, muted ink and every verdict ink keep their contrast floors;
 *   - the accent stays a fill in light mode (2.2:1 there) and only its ink is
 *     ever used as text — the pink block is a highlight, never words;
 *   - focus is visible (≥ 3:1 against the canvas);
 *   - both themes define the same tokens, so a component cannot find `--success`
 *     in one theme and not the other;
 *   - the semantic names components consume are exposed through `@theme`.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const css = readFileSync(fileURLToPath(new URL('../styles/global.css', import.meta.url)), 'utf8')
  // Comments explain the palette and contain colons; strip them before parsing.
  .replace(/\/\*[\s\S]*?\*\//g, '');

type Tokens = Record<string, string>;

/** The declarations of one rule block, e.g. `:root { --a: #fff; }`. */
const block = (selector: string): Tokens => {
  const start = css.indexOf(`${selector} {`);
  if (start === -1) {
    throw new Error(`no ${selector} block in global.css`);
  }
  const body = css.slice(start, css.indexOf('\n}', start));
  // Drop the selector line, which would otherwise swallow the first declaration.
  const declarations = body.slice(body.indexOf('{') + 1);
  const tokens: Tokens = {};
  // One declaration per `;`, then name/value on the first `:`. A regex over the
  // whole block backtracks badly on a long declaration.
  for (const declaration of declarations.split(';')) {
    const colon = declaration.indexOf(':');
    if (colon === -1 || !declaration.trimStart().startsWith('--')) {
      continue;
    }
    const name = declaration.slice(0, colon).trim().slice(2);
    tokens[name] = declaration.slice(colon + 1).trim();
  }
  return tokens;
};

const light = block(':root');
const dark = block('.dark');

describe('the file parses as this test assumes', () => {
  it('finds both themes and their first declarations', () => {
    // Guards the parser itself: a selector line that swallowed `--background` was
    // exactly the bug that let a broken palette pass here once.
    expect(light.background).toBeDefined();
    expect(dark.background).toBeDefined();
    expect(light.radius).toBeDefined();
    expect(Object.keys(light).length).toBeGreaterThan(20);
  });
});

/** sRGB channels of `#rrggbb`, or `null` for a value this parser does not cover. */
const rgb = (value: string): [number, number, number] | null => {
  const match = /^#([0-9a-f]{6})$/i.exec(value);
  if (!match) {
    return null;
  }
  const n = Number.parseInt(match[1] ?? '', 16);
  return [(n >> 16) & 0xFF, (n >> 8) & 0xFF, n & 0xFF];
};

const channel = (c: number): number => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};

const luminance = (value: string): number => {
  const channels = rgb(value);
  if (!channels) {
    throw new Error(`${value} is not a hex colour this test can measure`);
  }
  const [r, g, b] = channels.map(channel) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

/** WCAG 2.x contrast ratio between two opaque colours. */
const ratio = (a: string, b: string): number => {
  const [x, y] = [luminance(a), luminance(b)];
  const hi = Math.max(x, y);
  const lo = Math.min(x, y);
  return (hi + 0.05) / (lo + 0.05);
};

/** A token that must exist; a missing one is the failure, not a type widening. */
const need = (tokens: Tokens, name: string): string => {
  const value = tokens[name];
  if (value === undefined) {
    throw new Error(`global.css does not define --${name}`);
  }
  return value;
};

const theme = (name: 'light' | 'dark') => (name === 'light' ? light : dark);

/**
 * The gradient colours live inside `@supports`, so they are read from there: that
 * is the value a capable browser paints, and the only one that exists.
 */
const gradients = (name: 'light' | 'dark'): Tokens => {
  const start = css.indexOf('@supports (color: color-mix(in oklab, white, black))');
  if (start === -1) {
    throw new Error('no @supports block for the gradient colours');
  }
  // The light values are its first `:root`, the dark ones its following `.dark`.
  const body = css.slice(start, css.indexOf('\n}', css.indexOf('.dark', start)) + 2);
  const selector = name === 'light' ? ':root {' : '.dark {';
  const from = body.indexOf(selector);
  if (from === -1) {
    throw new Error(`no ${selector} inside @supports`);
  }
  const block = body.slice(from, body.indexOf('}', from));
  const tokens: Tokens = {};
  for (const line of block.slice(block.indexOf('{') + 1).split(';')) {
    const colon = line.indexOf(':');
    if (colon === -1 || !line.trimStart().startsWith('--')) {
      continue;
    }
    tokens[line.slice(0, colon).trim().slice(2)] = line.slice(colon + 1).trim();
  }
  return tokens;
};

/** The alpha a gradient token actually mixes, as a 0..1 fraction. */
const mixStrength = (value: string): number => {
  const percent = /color-mix\([^;]*?([\d.]+)%/.exec(value)?.[1];
  if (percent === undefined) {
    throw new Error(`not a colour mix with a percentage: ${value}`);
  }
  return Number.parseFloat(percent) / 100;
};

describe.each(['light', 'dark'] as const)('%s theme', (name) => {
  const t = theme(name);
  const canvas = need(t, 'background');
  const card = need(t, 'card');

  it('keeps body ink far above AA', () => {
    expect(ratio(need(t, 'foreground'), canvas)).toBeGreaterThanOrEqual(12);
    expect(ratio(need(t, 'card-foreground'), card)).toBeGreaterThanOrEqual(12);
  });

  it('keeps muted ink at the floor for small text (6:1)', () => {
    // ITEM-124: labels and captions are 13–15px, so they are small text and the
    // floor is 6:1 rather than 4.5:1.
    expect(ratio(need(t, 'muted-foreground'), canvas)).toBeGreaterThanOrEqual(6);
    expect(ratio(need(t, 'muted-foreground'), card)).toBeGreaterThanOrEqual(6);
  });

  it('keeps every verdict ink readable on the surfaces it lands on', () => {
    for (const ink of ['destructive', 'warning', 'success'] as const) {
      expect(ratio(need(t, ink), canvas), `${ink} on the canvas`).toBeGreaterThanOrEqual(4.5);
      expect(ratio(need(t, ink), card), `${ink} on a card`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('uses the accent as a fill and its ink as text', () => {
    // The signature pink is 2.2:1 on the light canvas: it may be a wash, a rule
    // or a dot, never a word. `accent-ink` is the text form and must pass AA.
    expect(ratio(need(t, 'accent-ink'), canvas)).toBeGreaterThanOrEqual(4.5);
    expect(ratio(need(t, 'accent-ink'), card)).toBeGreaterThanOrEqual(4.5);

    if (name === 'light') {
      expect(ratio(need(t, 'accent'), canvas)).toBeLessThan(4.5);
    }
  });

  it('makes focus visible', () => {
    // WCAG 2.2 focus appearance: at least 3:1 against the adjacent colours.
    expect(ratio(need(t, 'ring'), canvas)).toBeGreaterThanOrEqual(3);
    expect(ratio(need(t, 'ring'), card)).toBeGreaterThanOrEqual(3);
  });

  it('gives the chart ramp a readable order', () => {
    // Series colours are drawn as 2px bars and 8px dots, not as text, so 3:1
    // against the surface is the bar (WCAG 1.4.11 for graphics).
    for (const series of ['chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5'] as const) {
      expect(ratio(need(t, series), card), `${series} on a card`).toBeGreaterThanOrEqual(3);
    }
  });
});

/**
 * ITEM-149: a gradient must not cost legibility.
 *
 * The strongest point of each gradient is its darkest, so that is where text is
 * measured: the accent composited over the canvas at the token's own strength. If
 * someone raises `--gradient-glow` to 40% the text over the hero stops being
 * readable, and this fails rather than shipping it.
 */
describe.each(['light', 'dark'] as const)('%s gradients', (name) => {
  const t = theme(name);
  const g = gradients(name);

  /** An sRGB alpha composite, which is close enough for a contrast floor. */
  const over = (base: string, top: string, alpha: number): string => {
    const [br, bg, bb] = rgb(base) as [number, number, number];
    const [tr, tg, tb] = rgb(top) as [number, number, number];
    const mix = (b: number, f: number) => Math.round(b * (1 - alpha) + f * alpha);
    return `#${[mix(br, tr), mix(bg, tg), mix(bb, tb)]
      .map(channel => channel.toString(16).padStart(2, '0'))
      .join('')}`;
  };

  it('keeps a wash over content readable for muted text', () => {
    // A wash covers the content area, where muted labels live, so the muted floor
    // is the one that binds. It is why this number is small: the accent has almost
    // no headroom against #5b616b on this canvas.
    const strength = mixStrength(need(g, 'gradient-wash-color'));
    const peak = over(need(t, 'background'), need(t, 'accent'), strength);

    expect(ratio(need(t, 'muted-foreground'), peak), 'muted ink over the peak').toBeGreaterThanOrEqual(6);
    expect(ratio(need(t, 'foreground'), peak), 'body ink over the peak').toBeGreaterThanOrEqual(12);
  });

  it('allows a glow to be stronger, because a glow sits behind a heading', () => {
    // The design language restricts a glow to a title band, where the text is body
    // ink. Putting muted text over it would breach the floor above, so the rule is
    // placement, not a weaker gradient.
    const strength = mixStrength(need(g, 'gradient-glow-color'));
    const peak = over(need(t, 'background'), need(t, 'accent'), strength);

    expect(ratio(need(t, 'foreground'), peak), 'body ink over the peak').toBeGreaterThanOrEqual(12);
  });

  it('keeps the card depth within a hairline, not a panel', () => {
    // The depth gradient is ink, not accent: it darkens or lightens the header.
    const strength = mixStrength(need(g, 'gradient-depth-color'));
    const peak = over(need(t, 'card'), need(t, 'foreground'), strength);

    expect(strength).toBeLessThanOrEqual(0.03); // a card header is a tint, not a shade
    expect(ratio(need(t, 'foreground'), peak)).toBeGreaterThanOrEqual(12);
    expect(ratio(need(t, 'muted-foreground'), peak)).toBeGreaterThanOrEqual(6);
  });

  it('has exactly three gradients, so the language stays small', () => {
    expect(Object.keys(g).sort()).toEqual([
      'gradient-depth-color',
      'gradient-glow-color',
      'gradient-wash-color',
    ]);
  });
});

describe('the two themes stay in step', () => {
  it('defines the same colour tokens in each', () => {
    // `radius` is geometry, and it is declared once on purpose.
    const colours = (t: Tokens) => Object.keys(t).filter(name => name !== 'radius').sort();

    expect(colours(dark)).toEqual(colours(light));
  });

  it('exposes the semantic names components consume', () => {
    // A component writing `text-success` needs `--color-success` in @theme, or
    // the class silently does nothing.
    for (const name of ['accent-ink', 'success', 'warning', 'destructive', 'ring']) {
      expect(css).toContain(`--color-${name}: var(--${name});`);
    }
  });
});
