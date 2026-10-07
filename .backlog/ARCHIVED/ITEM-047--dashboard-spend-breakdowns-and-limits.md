## ITEM-047: Dashboard — provider/model/key spend and spend controls

**As a** tenant owner
**I want** to see spend broken down the way I think about it, and cap it
**So that** the dashboard answers "who/what is costing me money, and how do I stop it"

The 2026-10-05 redesign added time-series charts, KPIs (cost/request, error rate,
TTFT), and period comparison. What is still missing is the *controllable* view: the
breakdowns that map to the things a tenant can act on, and limits.

**Acceptance Criteria:**
1. Spend/usage breakdown by **provider**, **model**, **API key**, and **system
   prompt**, each over the selected window and comparable to the prior period.
2. Per-key and per-provider spend **limits** with alerting (reuse ITEM-020 alerts).
3. Cost anomalies surfaced inline (which model/key spiked) rather than only in alerts.
4. A clear empty/onboarding state for a brand-new tenant (what to do first).

**Technical Notes:**
- Extends the existing dashboard JSON APIs (summary/timeseries/by-model/by-prompt)
  with by-provider and by-key dimensions (both already exist as join keys:
  `providers`, `api_keys`).
- Depends on ITEM-043/044 (provider + model concepts) for the provider/model views.
- The chart/metric primitives from the redesign (`dashboardMetrics.ts`, `LineChart`)
  are reused — this is new queries + panels, not a new chart stack.

**Source:** PO review 2026-10-06 — "the dashboard doesn't show anything useful".

## Progress (2026-10-06) — spend breakdowns + limits landed

- **Data model** (`db/init/017_trace_dimensions_and_limits.sql`): `traces.provider_id`
  + `traces.api_key_id` (both `NULL` for shared-key / legacy traffic) with
  tenant-scoped indexes; `api_keys.spend_limit_cents` + `providers.spend_limit_cents`
  (0 = no limit); `alerts.class` widened to `TEXT` for per-entity classes.
- **Attribution** (`src/tenant.rs`, `src/providers.rs`, `src/lib.rs`,
  `src/telemetry.rs`): auth resolves the `api_keys` row (`Tenant.api_key_id`),
  provider resolution returns the provider id (`Route.provider_id`); both are
  threaded through `RequestMeta`/`TraceMessage` into the `traces` sink.
- **APIs** (`src/dashboard.rs`): `GET /api/by-provider` and `GET /api/by-key`
  (window + `offset` for period comparison; LEFT JOIN resolves labels + limits),
  `POST /api/keys/:id/limit` (owner-only), `spend_limit_cents` on the key and
  provider payloads.
- **Alerting** (`src/alerts.rs`): month-to-date spend is checked against each
  key/provider limit; an episode per entity (`key_spend_limit:<id>` /
  `provider_spend_limit:<id>`) fires/re-fires and clears when back under the cap,
  reusing the ITEM-020 lifecycle + webhook.
- **UI**: overview gains **Spend by provider** and **Spend by API key** tables
  with period deltas, an inline **Spike** badge (AC3) and an editable monthly
  cap per row (owner-only) — `SpendBreakdown.tsx`; a clear onboarding empty state
  (AC4) links to connect/providers/models.
- Rust 206 → **209** tests; web **50 → 53** unit; tsc/eslint/knip/i18n +
  `next build --webpack` green.
