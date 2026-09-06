## ITEM-025: Schema Guard — Transparent Fallback Circuit (Slice 4 · was 021c)

**As a** developer on the proxy
**I want** a failed cheap-model response to silently become a frontier-model response
**So that** the end user never sees an error or a malformed body

**Acceptance Criteria:**
1. On any failure classified by ITEM-024: the bad buffered response is discarded and the original prompt is replayed to the tenant's frontier model with the tenant's upstream key; the replacement response streams to the client
2. The client receives exactly one response — a latency spike on that request, never an error, never a duplicated/partial body
3. Exactly one trace is emitted for the request, recording the guard outcome + failure reason (the fallback is not a second request in tenant accounting)
4. If the fallback upstream itself fails, the request fails loudly and cleanly (no silent data loss) with the fallback reason preserved on the trace

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021c
- Depends on ITEM-023 + ITEM-024
- Open decision from parent: the frontier model defaults to the tenant's *original requested model* (pre-ITEM-015 downgrade — the correct "frontier" in a DeepSeek-only world); an explicit per-tenant configured frontier is the alternative — confirm before building

## Verification record (2026-09-06)

- **Decision (open item, applied)**: the replay target is the tenant's
  *originally requested model* — pre-router/budget downgrade — when that is a
  frontier model (their own frontier in a single-upstream world). When the
  original request itself named a cheap-tier model (a replay to the same
  failing model would fail the guard again), the replay escalates to
  `deepseek-v4-pro` (`guard::DEFAULT_GUARD_FRONTIER`, the strongest model the
  default upstream serves; env-configurable later if the PO asks). No
  per-tenant frontier config was added — nothing new to provision or migrate
  on `tenant_configurations`.
- **AC1 (discard + replay original prompt, tenant key)**: `check_guarded_sse`
  rejection (or a guarded stream dying before bytes — the "connection crash"
  class) routes to `fallback_to_frontier`: the original pre-override payload
  is reserialized with the frontier model and re-sent to the same upstream
  with the tenant's upstream key; the replacement response is served to the
  client. Trace model is retargeted to the model that served (the cost basis
  the telemetry worker prices on).
- **AC2 (exactly one response)**: the failed cheap buffer never reaches the
  client — the client sees one 200 SSE body equal to the frontier replay,
  byte for byte (never a partial/duplicated cheap body).
- **AC3 (exactly one trace + outcome + reason)**: `BufferedGuard` stays
  one-shot across the whole fallback (incl. client abort mid-replay — Drop
  emits exactly one truncated trace, event included). The trace's
  `guard_event` JSONB (migration 004, additive) records
  `{"outcome":"fallback","reason":...,"guarded_model":...}`. A mid-replay
  abort test pins the one-trace invariant.
- **AC4 (fallback failure is loud and clean)**: replay transport error or
  non-2xx answers 502 `guard_fallback_error` (never silent); the single trace
  records `{"outcome":"fallback_failed", ...,"detail":"http 500"|"transport error"}`
  preserving the original guard reason.
- Scope boundary (recorded): a pre-guard 5xx from the cheap upstream is
  forwarded as-is by the existing error path (it carries no body to
  validate — the provider's own error is the honest answer); the guard
  circuit covers every failure the guard itself classifies. Guarded flush-ok
  outcome recording + dashboard surfaces land in ITEM-026. 123 tests green.
