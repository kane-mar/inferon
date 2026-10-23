## ITEM-077: Local / edge models are priced at the cloud default

**As a** tenant serving my own model through an edge node
**I want** its inference to cost zero (or a price I set)
**So that** dashboards and budget caps don't show phantom spend

**Found:** 2026-10-23, on the first real edge node (LM Studio, port 1234). A
request served by `huihui-deepseek-v4-flash-0731-abliterated` traced with
`no price row — costed at the default $3.00/$15.00 per 1M`, so the dashboard
reports spend for inference that costs nothing at the margin — and a monthly
budget can trip on it.

**Why:** `cost::price_for` is keyed by model id with a global default fallback;
nothing marks a provider/model as locally hosted.

**Acceptance Criteria**
1. A provider (or model) can be marked **locally hosted / unpriced**; traces it
   serves record `upstream_cost_usd = 0`.
2. The edge node marks its local provider unpriced by default (no operator step).
3. Cost and savings views do not count local inference as spend, while savings
   versus the frontier can still be attributed.
4. `unpriced` stays a distinct, visible counter for models that are genuinely
   unknown (not conflated with local, which is deliberately free).

## Close-out (2026-10-23)

- `providers::is_local(kind)` + a `local` provider kind; the data plane threads
  locality `Route` → `RequestMeta` → `TraceMessage.is_local`.
- The edge node sends `is_local: true` on every trace (its provider is on-prem by
  definition); `EdgeTrace` propagates it into the cloud pipeline.
- The telemetry worker costs a local trace at **zero**, zeroes cache savings,
  still attributes the full counterfactual as savings, and **skips**
  `warn_if_unpriced` — so `unpriced` stays a distinct signal for genuinely
  unknown models (AC4).
- `cost::trace_cost` / `trace_cache_savings` / `trace_savings` are unit-tested.
- **Live** (edge → LM Studio, `huihui-deepseek-v4-flash-0731-abliterated`,
  58 → 15 tokens): `upstream_cost_usd = 0.000000`, `cache_savings_usd = 0`,
  `unpriced_models = 0` — previously the same call traced at the $3/$15 default.
- Rust **310 → 312** tests, clippy clean. Commit `2002627`; CI `39922782240`.
