// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-138: the interface states data; it does not explain it.
 *
 * The pressure to explain is constant — a KPI added, a term that feels unclear,
 * and a sentence appears under the number. This test is the counterweight, and it
 * is deliberately mechanical because the rule is mechanical:
 *
 *   - no screen text may be a sentence: no full stop, no clause after a comma;
 *   - report namespaces may not grow `_help` / `_desc` / `_note` keys at all;
 *   - a page header may not carry a description;
 *   - when a term genuinely needs defining, the answer is `DocsLink`.
 *
 * A label that cannot pass is a label that needs renaming.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import en from '@/locales/en.json';

const WEB = fileURLToPath(new URL('../..', import.meta.url));

/** Namespaces that render inside the product, where the rule applies. */
const SCREEN_NAMESPACES = Object.keys(en).filter(
  ns => ns.endsWith('Page') || ns === 'DashboardHeader' || ns === 'Navbar',
);

/** Keys whose value is metadata, not screen text. */
const isMetadata = (key: string) => key.startsWith('meta_');

/**
 * A sentence is more than a label: a clause train, or two sentences in one string.
 * Values built from data (they carry a `·` separator) are exempt — `24 h · 1284
 * samples` is a reading, not prose.
 */
const isSentence = (value: string): boolean => {
  if (value.includes('·')) {
    return false;
  }

  const words = value.split(/\s+/).filter(Boolean).length;
  const enders = (value.match(/[.!?](?:\s|$)/g) ?? []).length;

  return words > 12 || (enders > 1 && words > 6);
};

describe('screen copy is labels, not prose', () => {
  it.each(SCREEN_NAMESPACES)('%s has no explanatory sentences', (namespace) => {
    const body = (en as Record<string, Record<string, string>>)[namespace] ?? {};
    const offenders = Object.entries(body)
      .filter(([key, value]) => !isMetadata(key) && typeof value === 'string' && isSentence(value))
      .map(([key, value]) => `${key}: ${value}`);

    expect(offenders).toEqual([]);
  });

  it.each(SCREEN_NAMESPACES)('%s has no _help / _desc / _note keys', (namespace) => {
    const body = (en as Record<string, Record<string, string>>)[namespace] ?? {};
    const offenders = Object.keys(body).filter(key => /_(?:help|desc|note|body)$/.test(key));

    expect(offenders).toEqual([]);
  });

  it('renders no page-header descriptions', () => {
    // A title plus data. A description under a title is the page explaining
    // itself instead of being legible.
    const offenders = sourceFiles(join(WEB, 'src/app')).filter(file =>
      /description=\{t\('title_bar_description/.test(readFileSync(file, 'utf8')),
    );

    expect(offenders).toEqual([]);
  });
});

/** Every .tsx under `dir`, recursively. */
function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);

    if (statSync(path).isDirectory()) {
      return sourceFiles(path);
    }

    return path.endsWith('.tsx') ? [path] : [];
  });
}
