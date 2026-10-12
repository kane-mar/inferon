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
