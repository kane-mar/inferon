## ITEM-110: Plain-language UI copy (grade-12 reading level)

**As a** reader of the landing page, the dashboard and the reports
**I want** the words to be ones a grade-12 student reads without effort
**So that** the idea lands before the reader gives up — cost, protection,
quality, speed — instead of having to decode the vocabulary first

**Context (2026-11-05, PO request).** The UI copy had grown dense: sentences
with three subordinate clauses, statistics named by their percentile (`P95
TTFT`), metaphor instead of description ("circuit breaker that degrades
safely", "money left on the table", "bill-shock attribution"), and product
vocabulary ("tenant", "schema guard", "payload", "holdout", "PII") used before
it was ever defined. This was not a backlog item; the PO asked for a copy pass
across the Inferon pages.

**Acceptance Criteria**
1. Every user-facing string in `web/src/locales/en.json` and `fr.json` reads at
   roughly a grade-12 level: one idea per sentence, common words, no metaphor
   standing in for a mechanism.
2. Product vocabulary is either replaced by its plain equivalent or defined at
   first use — tenant → workspace, schema guard → output checker, payload waste
   → waste warnings, holdout → comparison group, PII → personal data.
3. Statistics are spelled out for a non-statistician: p50 → median, p95 → "the
   slowest 5%", with the definition ("95 out of 100 responses started faster
   than this") in the hover help.
4. Technical identifiers stay exact and are glossed, never hidden: endpoint
   paths, `proxy_us`, `max_tokens`, 4xx/5xx, triggers, model ids.
5. Key structure, ICU placeholders and rich-text tags are unchanged, so no
   component and no test that reads copy breaks beyond the hero selector.
6. Both locales move together: the French copy carries the same simplifications.
7. Checks green: `i18n-check` (no missing/unused/undefined keys, no invalid
   ICU), `tsc --noEmit`, unit tests, `next build`.

**Technical Notes**
- Copy lives only in the locale files; components read it through `next-intl`
  (`t()` / `t.rich()`), so no component changed.
- `Hero.title` keeps its `<important>` tag; only the wording inside changed.
- The two Playwright specs that assert hero text
  (`tests/e2e/I18n.e2e.ts`, `tests/e2e/Sanity.check.e2e.ts`) follow the new title.
- Terminology has to stay consistent across 650 keys per locale: one pass over
  the whole file, not per-page edits, or the same concept drifts.

**Source:** PO request, 2026-11-05 ("review the copy on the inferon pages …
make the language suitable for grade 12").

## Delivered (2026-11-05)

- 274 of 650 EN strings and 257 FR strings rewritten; keys, placeholders and
  the hero rich tag verified unchanged against the previous commit.
- Jargon → plain term: tenant → workspace, schema guard → output checker,
  heuristic router → automatic router, holdout → comparison group, circuit
  breaker → budget protection, "degrade safely" → switches to a cheaper model,
  attribution → what caused the bill, append-only → never edited or deleted.
- Statistics: p50 → median, p95 → "slowest 5%", p99 → "slowest 1%", with the
  percentile defined in each help string.
- Deployed: `inferon-web` rebuilt on the durable host; homepage EN + FR show the
  new copy, 15 dashboard pages 200 with a real session, a real completion 200,
  no proxy panics and no web errors in the deploy window.

### Documentation (same request)

- `README.md`, `guides/inferon-self-hosted.md`, `guides/inferon-cost-audit.md`,
  `guides/inferon-step2-materials.md` and `web/README.md` rewritten to the same
  standard: one idea per sentence (mean 16–21 words), plain verbs, no metaphor
  standing in for a mechanism. Every command, path, env var, endpoint, number and
  table is unchanged, so the docs still match the code.
- Prose now says **workspace** and keeps the identifiers that say `tenant`
  (`tenants` table, `seed-tenant.sh`, `/v1/dashboard/api/*`), with a note at the top
  of the README so the two do not look like different things.
- Jargon replaced throughout "attribution" → explaining where the money went,
  "moat/wedge" → the gap competitors do not cover, "holdout" → comparison group.
- Checked: average sentence length per file (README 20.0, self-hosted 15.7,
  step 2 19.0, cost audit 20.7 words), balanced code fences and inline code,
  relative links resolve.
