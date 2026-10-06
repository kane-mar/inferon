## ITEM-045: Fallback & routing rules

**As a** tenant owner
**I want** to define what happens when a model fails
**So that** traffic degrades gracefully instead of surfacing a provider error to my users

The schema guard already does a *frontier replay* for schema violations (ITEM-025),
but there is no general, tenant-configurable fallback for transport/HTTP failures,
timeouts, or rate limits. A real deployment needs "try primary, then X, then Y".

**Acceptance Criteria:**
1. A dashboard page (`/dashboard/routing`) lets the tenant define an ordered
   fallback chain: primary model → fallback models, with per-step triggers
   (upstream error, timeout, rate-limit / 429, schema-guard violation).
2. The proxy applies the chain on the request path; the first step that succeeds wins.
3. Fallback events are recorded as telemetry and surface on the trace + dashboard
   (which model actually served the request, and why it changed).
4. A chain exhausts to a clear typed error; it never loops.
5. With no chain configured, behavior is unchanged (single model, current fallback).

**Technical Notes:**
- Extend the existing guard fallback seam (`guarded_stream_response` /
  `fallback_to_frontier`) into a general `routing_policy` (ordered steps + triggers).
- Depends on ITEM-044 (the chain references enabled models) and pairs with ITEM-046
  (the policy UI shares the routing page).
- Keep the hot path allocation-light: the policy is loaded once, not per request.

**Source:** PO review 2026-10-06 — "no rules for fall back".

## Progress (2026-10-06) — fallback chains landed

- `src/routing.rs`: `RoutingStore` (Postgres + in-memory) + `fallback_reason`
  (429 / 5xx / transport are retryable; any other 4xx never is).
- migration 014: `routing_policies` (tenant, ordered `fallbacks` JSONB) + a
  `traces.routing_event` column.
- **Proxy**: a request now walks the ordered chain — the primary (after any
  router/budget override), then the tenant's fallbacks (de-duped, disabled
  models dropped per ITEM-044) — and records `{from_model, to_model, reason}` on
  the trace (AC2/AC3). An exhausted chain serves the last error (forwarded as-is,
  or 502 on transport). An empty chain changes nothing (AC5, default).
- API: `GET`/`POST /v1/dashboard/api/routing` (owner-only set).
- UI: `/dashboard/routing` + `RoutingPanel` (ordered chain: add a catalog model,
  remove) + a nav item; en/fr copy.
- Rust 199 tests (incl. a 429→fallback→200 integration test asserting the traced
  event), web 48, all checks + `next build` green.
- **Deferred**: per-step triggers (all retryable classes share one chain) and the
  schema-guard trigger (the guard keeps its own frontier replay, unchanged).
