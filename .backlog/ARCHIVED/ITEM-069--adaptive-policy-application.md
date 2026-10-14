## ITEM-069: Adaptive policy application — provider weights & task-cluster preference

**As a** tenant
**I want** the adaptive policy (ITEM-064) to also tune *which provider/model*
serves a task, not just the router threshold
**So that** the closed loop exploits provider health and per-task quality, not
only complexity

Split from ITEM-064 (2026-10-14): the loop and its governance are delivered
(versioned/auditable/reversible policy, bounded step, kill switch, rollback,
`traces.policy_version`). This item extends what the policy *contains* and what
the hot path does with it.

**Acceptance Criteria:**
1. **Provider weights** derived from per-provider health (ITEM-061: error rate,
   p95) are recorded in the policy and applied when several providers could serve
   a model / fallback — a degrading provider is demoted, not blindly used.
2. **Per task-cluster model preference** (ITEM-029 clusters + ITEM-055 quality)
   is recorded in the policy and consulted by the router/solver, so "this cheap
   model is good enough for cluster X" is learned from data.
3. All of it inherits ITEM-064's guardrails: bounded change, frontier/fallback
   retained, holdout-gated exploration, kill switch, rollback.
4. Adaptation is **measured**: quality/cost/latency before vs after a policy
   version, so a bad application is detected and rolled back.

**Technical Notes:**
- Depends on staged ITEM-029 (task centroids) and ITEM-030 (Lagrangian solver);
  this item is the *policy surface and hot-path application*, not the clustering
  or the solver themselves.
- Keep the policy JSONB additive (new fields default to "no opinion") so an old
  hot path ignores fields it does not know.

**Source:** 2026-10-12 adaptive-operation analysis; ITEM-064 split 2026-10-14.

## Close-out — slice 1 (2026-10-14): provider health weights

- `policy::provider_weight`: quality × latency (relative to the fleet median),
  clamped to `[0.1, 1.0]`, **neutral below the sample floor** — too little data is
  no opinion, never a silent penalty.
- `analyse_providers` derives per-provider weights from the trailing week's
  telemetry (error rate + p95); `run_adaptation` publishes them alongside the
  threshold (`tenant_policies.provider_weights`, migration 033).
- `providers::providers_for_model` lists every provider that owns a model;
  `resolve_route` picks the **highest-weighted** one when the policy has weights
  (a degrading provider is demoted), else the static ordering.
- Inherits ITEM-064's guardrails (bounded, kill switch, rollback) and outcomes
  stay attributed via `traces.policy_version`.
- Rust **298 → 301** tests, clippy clean. Commit `b586d66`; CI `38655406264`
  (test+web+deploy) green.
- **Live**: policy **v8** for kane-mar carries
  `provider_weights = {…: 1.0, 5c63dbbb: 0.407, …}` — the degrading Deepseek
  provider (36% errors, higher p95) was demoted while the small-sample providers
  stayed neutral at 1.0.
- **Observed**: over several ticks the adapted threshold walked to ~0 (bounded,
  but a non-zero floor may be preferable — a tuning consideration, not a defect).
- **AC2 (task-cluster model preference) is split to ITEM-072** — it depends on
  the staged ITEM-029/030.
