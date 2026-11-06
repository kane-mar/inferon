## ITEM-125: The page explains itself — no documentation required

**As a** reader of a report
**I want** the screen to name things in words I use
**So that** "if the user needs the docs, the design failed" (PO, 2026-11-06)

**Context (PO, 2026-11-06).** The pages print raw API vocabulary and hide the meaning
in a hover title. Counted in the code:

- **Quality → Errors by cause** prints the stored class verbatim: `upstream_429`,
  `rate_limited`, `inferon_4xx`, `transport`. The guides (ITEM-117) teach these words
  because the screen will not; that is the failure this item removes.
- **The public cost-audit report** names its top failure the same way.
- **The operator console** prints guard outcomes `flushed`, `fallback`,
  `fallback_failed` — internal state names, not outcomes.
- **KPI definitions exist only in a `title` attribute** (hover). Touch devices never
  see them, and nobody hovers a card they do not know is hoverable.
- Two help texts explain a metric by naming the stored field (`proxy_us`).

**Acceptance Criteria**
1. One shared mapping (`src/libs/labels.ts`) turns every error class and outcome into a
   plain phrase plus one line of "what it means for you", with the raw code kept only as
   a secondary hint (small, muted, in the same cell). Used by Quality, the audit report
   (both surfaces) and the operator console — one vocabulary, one place to change it.
2. An unknown class still renders: it falls back to the raw code, never to a blank cell.
3. KPI definitions are visible text on the card; hover adds nothing that the card does
   not already say.
4. Copy that explains a metric by naming an internal field is rewritten in user terms.
5. `en` and `fr` both carry the new keys; `i18n-check` passes; a unit test covers the
   mapping including the unknown-class fallback.
6. Live: the Quality page shows phrases, not codes; the operator console shows outcomes
   in words.

**Technical Notes**
- The mapping is a pure function over a code string, so it is testable without a
  browser; the translation lives in the locale files under two namespaces (`ErrorClass`,
  `Outcome`).
- Keep the raw code visible: support replies and log lines use it, and a reader who
  wants the exact string should not have to guess.

**Source:** PO request, 2026-11-06.
