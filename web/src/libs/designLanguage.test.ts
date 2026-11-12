// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-150: the design language, as rules a build can check.
 *
 * `wiki/llm-inference/inferon-design-language.md` says a rule that is not in a test
 * is a preference, and preferences drift. These are the mechanical ones: type,
 * colour exclusions, motion, shape and section rhythm. Contrast lives in
 * `contrast.test.ts` and copy in `minimalCopy.test.ts`.
 *
 * Every failure names the file and line, so the fix is obvious.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const WEB = fileURLToPath(new URL('../..', import.meta.url));

type Source = { path: string; text: string };

const walk = (dir: string): string[] =>
  readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);

    if (statSync(path).isDirectory()) {
      return entry === 'node_modules' ? [] : walk(path);
    }

    return /\.(?:tsx|ts|css)$/.test(path) ? [path] : [];
  });

/** The first-party sources, minus the tests themselves. */
const sources = (): Source[] =>
  walk(join(WEB, 'src'))
    .filter(path => !path.endsWith('.test.ts'))
    .map(path => ({ path: path.replace(`${WEB}/`, ''), text: readFileSync(path, 'utf8') }));

/** Comment lines talk about the rules; they are not violations of them. */
const isComment = (line: string): boolean => /^\s*(?:\*|\/\*|\/\/)\s/.test(line);

/** `file:line text` for every line matching `pattern`. */
const offenders = (pattern: RegExp): string[] =>
  sources().flatMap(source =>
    source.text
      .split('\n')
      .map((line, index) => ({ line, number: index + 1 }))
      .filter(({ line }) => !isComment(line) && pattern.test(line))
      .map(({ line, number }) => `${source.path}:${number} ${line.trim().slice(0, 90)}`),
  );

describe('type', () => {
  it('sets every label in sentence case, without tracking', () => {
    // Uppercase 12px mono with wide tracking was the least readable combination
    // this product shipped. It came back once; this is why it cannot again.
    expect(offenders(/\b(?:uppercase|tracking-wid(?:e|er|est))\b/)).toEqual([]);
  });

  it('never sets type below 13px', () => {
    expect(offenders(/text-\[(?:\d|1[0-2])px\]/)).toEqual([]);
  });
});

describe('colour', () => {
  it('never paints text with the raw accent', () => {
    // The accent is 2.2:1 on the canvas: a fill, a rule or a dot, never words.
    expect(offenders(/\btext-accent(?!-)/)).toEqual([]);
  });

  it('never clips text into a gradient', () => {
    expect(offenders(/bg-clip-text/)).toEqual([]);
  });
});

describe('motion', () => {
  it('never animates geometry', () => {
    expect(offenders(/transition-all/)).toEqual([]);
  });

  it('keeps every transition short', () => {
    expect(offenders(/\bduration-(?:[3-9]\d{2}|\d{4,})\b/)).toEqual([]);
  });

  it('only transitions colour, opacity, transform or shadow', () => {
    // CSS longhands (`transition-duration`) and the reduced-motion opt-out are not
    // property lists.
    const allowed = /transition-(?:colors|opacity|transform|shadow|none)\b/;
    const longhand = /transition-(?:duration|property|delay|timing-function)\b/;

    expect(
      offenders(/\btransition-[a-z]/).filter(line => !allowed.test(line) && !longhand.test(line)),
    ).toEqual([]);
  });
});

describe('shape', () => {
  it('keeps radii to the four in the language, plus a full round', () => {
    // Cards 8px, controls 6px, chips 4px, chart cards 12px (`rounded-chart`);
    // `rounded-full` is a dot or a pill. An arbitrary radius is the thing banned.
    expect(offenders(/\brounded-(?:xl|2xl|3xl|\[)/)).toEqual([]);
    expect(offenders(/\brounded-chart\b/).length).toBeGreaterThan(0);
  });
});

describe('space', () => {
  it('gives every page section the same separation', () => {
    // The rhythm is 6 between sections. Only top-level children count: they sit at
    // six spaces of indentation, while anything deeper is inside a card and may use
    // the tighter spacing a control needs.
    const violations = sources()
      .filter(source => source.path.endsWith('/page.tsx'))
      .flatMap(source =>
        source.text
          .split('\n')
          .map((line, index) => ({ line, number: index + 1 }))
          .filter(({ line }) => /^ {6}<div className="mt-/.test(line))
          .filter(({ line }) => !/^ {6}<div className="mt-6\b/.test(line))
          .map(({ line, number }) => `${source.path}:${number} ${line.trim().slice(0, 70)}`),
      );

    expect(violations).toEqual([]);
  });
});
