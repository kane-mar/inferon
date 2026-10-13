## ITEM-057: Error taxonomy & truncation visibility

**As an** operator and tenant
**I want** failures classified by cause, not one undifferentiated "error" bucket
**So that** I can see per-provider health, distinguish my 4xx from an upstream
outage, and know when responses were cut off mid-stream

Today "errors" is a single count of `status_code >= 400`. Upstream 429 vs 5xx vs
transport 502 vs our own typed 400s (`no_provider`, `model_disabled`, guard) are
indistinguishable. `traces.truncated` is **persisted but never queried**, so
aborted/stream-cut responses are invisible.

**Acceptance Criteria:**
1. Each failed trace carries an **error class** (e.g. `upstream_429`,
   `upstream_5xx`, `transport`, `client_4xx`, `inferon_4xx`, `guard`, `timeout`)
   — stored or derivable on query.
2. The summary breaks the error rate down **by class and by provider**, and the
   operator view shows per-provider error rate.
3. The **truncation/abort rate** (`truncated`) is surfaced in the summary and
   attributed (client disconnect vs proxy timeout vs upstream cut).
4. The rate of each class is **alertable** (reuse ITEM-020).
5. Existing error semantics don't change for tenants — this is classification and
   visibility, not new rejection behaviour.

**Technical Notes:**
- The fallback path already classifies retryable failures (429/5xx/transport) —
  reuse that classification when writing `error_class`.
- Map our typed 400s at their construction site so classification isn't
  string-matching a message.
- Add `truncated` to the summary/count queries; it is already a column.

**Source:** 2026-10-12 metrics review — reliability has no taxonomy and abort
data is dead.

## Close-out (2026-10-13)

- `src/errors.rs`: `ErrorClass` (upstream_429 / upstream_5xx / client_4xx /
  inferon_4xx / inferon_5xx / transport / timeout / guard / client_abort),
  chosen at the failure site (never string-matching). Persisted as
  `traces.error_class` (migration 026, partial index); edge traces may carry
  their own.
- Wired at every failure site: `forward_buffered` (upstream status), transport
  (`reqwest::Error::is_timeout`), budget/model-disabled (`inferon_4xx`), guard
  failures, and stream/guard aborts (`client_abort`).
- Visibility: `/api/summary` gains `truncated`, `truncated_rate`,
  `errors_by_class`; `/api/quality` gains `truncated_rate`, `errors_by_class`;
  by-provider rows gain `errors` + `error_rate` (AC2).
- Web: the quality page shows a truncation KPI and an errors-by-class table.
- Rust **270 → 275** tests, clippy clean; tsc/eslint/i18n green. Commit
  `13b5942`; CI `38404578001` (test+web+deploy) green.
- **Live**: an upstream 400 was classified `client_4xx` and surfaced in
  `errors_by_class` with `truncated: 0`. Pre-026 rows have a NULL class (they
  predate the taxonomy), mirroring the outcome `unclassified` handling.
- **AC4 (alertable) split to ITEM-068** — the data is now available to the alert
  engine; the error-rate alert itself is the follow-up.
