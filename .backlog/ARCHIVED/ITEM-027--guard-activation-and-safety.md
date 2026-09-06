## ITEM-027: Schema Guard — Activation + Regression Safety (Slice 4 · was 021e)

**As a** product owner
**I want** the guard on by default for cheap-tier structured requests without ever regressing existing tenants
**So that** the safety net ships without a migration scare

**Acceptance Criteria:**
1. Guard is active by default for requests that carry a JSON-schema contract and route to the cheap tier; a per-tenant opt-out flag lives on `tenant_configurations`
2. Pure-passthrough requests (no `response_format` or not routed) show zero behavior change — verified by the existing smoke surface
3. `scripts/smoke.sh` gains a guarded-failure scenario: a dead/lying guarded upstream forces a fallback and the client still receives a valid 200
4. Hot-path budget verified on the durable host: <5ms added on guarded traffic (Slice 4 latency target), none on unguarded; no TTFT regression on the happy path beyond the buffered flush

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021e — the guard item's completion slice
- Depends on ITEM-023..026
- Open decision from parent: default scope = all cheap-tier routes vs opt-in until ITEM-018 brings real traffic shapes — the smoke scenario above assumes default-on

## Verification record (2026-09-06)

- **Decision (open item, applied)**: default-on for every cheap-tier structured
  request; opt-out is per tenant (`tenant_configurations.guard_enabled`, a
  config read failure fails open to guarded — never weakens the net).
- **AC1 (default on + per-tenant opt-out)**: the guarded branch now ANDs the
  existing `is_guarded_request` with `guard_enabled_for(tenant)` (migration
  005, applied live). Seam test: an opt-out tenant receives the lying cheap
  body byte-for-byte with no guard event; the smoke seeds a real opt-out
  tenant and asserts the same end-to-end.
- **AC2 (passthrough zero change)**: unguarded streaming keeps the zero-copy
  path — existing byte-for-byte tests + the full smoke surface pass unchanged
  (guard_event stays None on unguarded traces — additive only).
- **AC3 (smoke guarded-failure scenario, live)**: `scripts/smoke.sh` now runs
  against a lying mock upstream (the same binary in `--mock-upstream` mode,
  compose profile `mock` — deployment unit stays one image). It asserts: a
  guarded request whose cheap model answers schema-violating SSE gets a
  transparent frontier replay with HTTP 200 and the schema-valid body (never
  the lying bytes), the opt-out tenant sees the raw cheap body, and the
  fallback guard event persists in the DB. ALL SMOKE CHECKS PASSED on the
  durable host.
- **AC4 (hot-path budget, durable host)**: measured on the durable host (1
  vCPU) against the local mock: guarded flush p50 319.7ms / mean 319.6ms vs
  an unguarded streaming baseline p50 321.4ms / mean 322.1ms (25 requests
  each) — guard cost ≈ 0ms beyond noise. The first measurement showed ~9ms
  of added latency dominated by recompiling the JSON schema per request;
  fixed with a bounded compiled-validator cache keyed by schema text
  (`guard.rs`, unit-tested). TTFT on the guarded happy path is the buffered
  flush by construction (Slice 4 design), nothing beyond it. Unguarded
  traffic has zero added cost (no validator path).
- Ops notes (recorded): the redeploy surfaced a base-image drift — the
  floating `rust:1.98` builder moved to a trixie base whose binaries need
  GLIBC_2.38, crashing on the distroless cc-debian12 runtime. Dockerfile now
  pins `rust:1.98-bookworm` (glibc 2.36, matches the runtime). The host is
  running a transitional trixie-built image on the cc-debian13 runtime until
  the next redeploy rebuilds on the pinned bookworm builder. Migrations 004 +
  005 applied live. 126 tests green.
