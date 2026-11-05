#!/usr/bin/env node
// Licence audit for the npm dependency tree — ITEM-111 AC3.
//
// Checks every package in web/package-lock.json against an allow-list of
// licences that can ship inside this AGPL-3.0-only repository. Runs in CI; see
// LICENSES.md for why each licence is on the list.
//
// Rule: every licence named in an SPDX expression must be allowed. That is
// stricter than SPDX semantics (an `OR` only needs one allowed arm), and it is
// deliberate — a stricter list is safer and easier to review. Exceptions are
// per package and must carry a reason.
//
// Usage: node scripts/check-npm-licenses.mjs [path/to/package-lock.json]

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const LOCK_PATH = process.argv[2] ?? fileURLToPath(new URL('../web/package-lock.json', import.meta.url));

const ALLOWED = new Set([
  'MIT',
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'ISC',
  'MPL-2.0',
  'LGPL-3.0-or-later',
  'BlueOak-1.0.0',
  '0BSD',
  'CC0-1.0',
  'CC-BY-3.0',
  'CC-BY-4.0',
  'Unlicense',
  'WTFPL',
  'Python-2.0',
]);

// Non-SPDX or missing licence metadata, verified by hand. Keyed by lockfile path.
const EXCEPTIONS = new Map([
  [
    'node_modules/format',
    { license: 'MIT', reason: 'declares MIT in the legacy licenses[] field; the tarball has no LICENSE file' },
  ],
  [
    'node_modules/jsonify',
    { license: 'Public Domain', reason: 'public-domain dedication; "Public Domain" is not an SPDX identifier' },
  ],
]);

// The exception's declared licence must itself be acceptable.
const ALLOWED_EXCEPTIONS = new Set(['MIT', 'Public Domain']);

const SPDX_OPERATORS = new Set(['AND', 'OR', 'WITH']);

/** Licence identifiers named in an SPDX expression, minus exception names. */
function licenseIds(expression) {
  const tokens = expression.replaceAll('(', ' ').replaceAll(')', ' ').split(/\s+/).filter(Boolean);
  const ids = [];
  let skipNext = false;
  for (const token of tokens) {
    if (skipNext) {
      skipNext = false;
      continue;
    }
    if (token === 'WITH') {
      skipNext = true;
      continue;
    }
    if (!SPDX_OPERATORS.has(token)) ids.push(token);
  }
  return ids;
}

const lock = JSON.parse(readFileSync(LOCK_PATH, 'utf8'));
const packages = Object.entries(lock.packages ?? {}).filter(([path]) => path !== '');

const violations = [];
const counts = new Map();

for (const [path, meta] of packages) {
  const exception = EXCEPTIONS.get(path);
  const expression = exception?.license ?? meta.license;

  if (!expression) {
    violations.push({ path, expression: '(none)', detail: 'no licence field' });
    continue;
  }
  counts.set(expression, (counts.get(expression) ?? 0) + 1);

  if (exception) {
    if (!ALLOWED_EXCEPTIONS.has(exception.license)) {
      violations.push({ path, expression, detail: `bad exception mapping for ${exception.license}` });
    }
    continue;
  }

  const offending = licenseIds(expression).filter((id) => !ALLOWED.has(id));
  if (offending.length > 0) {
    violations.push({ path, expression, detail: `not allowed: ${offending.join(', ')}` });
  }
}

console.log(`checked ${packages.length} npm packages in ${LOCK_PATH}`);
for (const [expression, count] of [...counts].sort((a, b) => b[1] - a[1])) {
  console.log(`  ${String(count).padStart(4)}  ${expression}`);
}

if (violations.length === 0) {
  console.log('npm licences ok');
  process.exit(0);
}

console.error(`\n${violations.length} package(s) with a licence that is not allowed:`);
for (const { path, expression, detail } of violations) {
  console.error(`  ${path} — ${expression} (${detail})`);
}
console.error('\nAdd the licence to ALLOWED in this script and justify it in LICENSES.md, or replace the dependency.');
process.exit(1);
