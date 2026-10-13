## ITEM-062: Product activation & retention metrics

**As the** operator (and product owner)
**I want** to see whether real developers are actually using Inferon
**So that** "put it in front of real developers" is measured, not assumed

ITEM-018 proved the **first** request. There is no ongoing signal: how many
tenants/developers are active, how much they use it, whether they come back, or
which keys are exercising the API. The operator view lists tenants and counts, but
not engagement or trends.

**Acceptance Criteria:**
1. **Active tenants / active keys** over a window (e.g. WAU/MAU), distinct from
   cumulative signups.
2. **Requests per active developer/key** and a week-over-week trend (growing,
   flat, churning).
3. **First-request funnel**: signed up → created a provider → first successful
   request → returned the next week.
4. Operator-only, tenant-anonymised aggregates (no cross-tenant payload leakage).
5. Per-key usage is visible to the tenant for their own keys (spend already is).

**Technical Notes:**
- Aggregations over `traces` joined to tenants/keys by `created_at` windows; the
  operator console already renders tenant aggregates.
- Distinct from billing: this measures engagement, not cost.
- Keep it aggregate and operator-scoped; no raw cross-tenant content.

**Source:** 2026-10-12 metrics review — only the first-request milestone is
captured; sustained usage is not.

## Close-out (2026-10-14)

- `GET /v1/dashboard/api/operator/activation` (operator-only) returns active
  tenants/keys, requests, and requests per active tenant/key for the window —
  each with the preceding window for a **week-over-week** trend (`*_wow`, null
  when there is no baseline).
- The **first-request funnel** over the window's signup cohort: `signed_up` →
  `created_provider` → `first_request` → `returned` (traced on ≥2 distinct days).
- Tenant-anonymised aggregates only (ids/counts; no cross-tenant content).
- Operator console renders an **Engagement** card and the funnel.
- AC5 (per-key usage for the tenant's own keys) is already delivered by the
  by-key spend view (requests + spend per key, ITEM-047).
- Rust **279 → 280** tests, clippy clean; tsc/eslint/i18n green. Commits `37b6ca6`
  + `b0e9192` (a live-caught unbound `$1` fix); CI `38484184284` green.
- **Live**: `active_keys` 14, `active_tenants` 2, 1776 requests,
  126.9 requests/active key; funnel `{signed_up: 2, created_provider: 1,
  first_request: 2, returned: 2}`; WoW null (the previous window had no traffic).
- **Lesson**: DB query bindings are not covered by the unit tests (no DB in the
  loop) — this item's unbound parameter was caught only by live verification.
