## ITEM-144: remove em dashes from everything a reader sees

**Requested** 2026-11-10: "remove the em-dash. I don't like."

**Delivered**

- **Guides: 178 → 1.** Headings take a colon, bold-term list items take the colon
  the guideline's own list format asks for, prose takes a colon before an
  explanation and a comma before a conjunction, the two table cells that needed a
  semicolon got one. The remaining one is the value placeholder inside backticks
  (`KPIs read 0 or —`), which is a data glyph rather than punctuation.
- **README: 29 → 0.**
- **UI copy: 14 per locale.** Short labels read better with a colon
  (`New key: copy it now`, `Connected: {count} models`).
- **Product messages: 7.** The gateway was answering with em dashes of its own
  (`no upstream provider configured; add one in Dashboard → Providers` now), plus
  the OAuth mismatch, the tenant-setup failure, the unpriced-cost warning and the
  embedder and webhook notices. The guides that quote those messages were updated
  to quote what the port actually answers, so the documentation stays true.
- **`check-guides.py` now fails on an em dash in prose**, with code spans exempt for
  the placeholder. Proven red by injecting one and green after removing it.

**Deliberately left alone:** em dashes in Rust doc comments and in `wiki/` notes.
Those are developer-facing, not written for a reader of the product, and 400
cosmetic edits would bury the diff that matters. One command finishes them if the
preference extends to source comments.

**Verified live:** 0 em dashes in the rendered dashboard text, eight pages 200,
changed messages deployed, completion 200, 0 errors.

## ITEM-145: the unit suite had not run for days, and it had findings

**Cause.** The SMB share lost a native binding's code signature, so `vitest` could
not start. Re-running `npm ci` on the share stalled at ~554 of 947 packages for 40
minutes; installing `node_modules` on local disk and symlinking it took 11 seconds.
The fresh install then turned out to be missing `next/dist/server/request/`, so the
published tarball was extracted over it.

**What the suite found when it finally ran** (three real defects, none visible to
tsc, eslint or i18n-check):

1. `session.test.ts` could never have passed: `next/headers` cannot be `require`d
   outside the bundler, so importing the module failed the whole file. The test now
   mocks the seam it does not use, which is what makes it a unit test rather than a
   test of Next's packaging.
2. `dashboardNav.test.ts` still described the pre-ITEM-140 menu. Runtime is the
   operator section's child, so the operator console has one tab and
   `/dashboard/runtime` resolves to that section.
3. `minimalCopy.test.ts` was right and the copy was not: 20 keys still carried
   `_help` / `_note` / `_body` in their names while holding short labels. Renamed so
   a name stops promising an explanation (`capture_off_help` → `capture_off`,
   `key_note` → `key_hint`, and so on), both locales and every call site.

**Result: 169 tests across 15 files green**, tsc clean, 599 Rust tests.

**#lesson.** A test suite that cannot run is worse than no suite: it holds the
regressions you believe you are protected from. The first thing this one did on
returning was disagree with three things I had "verified" by eye.

**Source:** PO request, 2026-11-10.
