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
