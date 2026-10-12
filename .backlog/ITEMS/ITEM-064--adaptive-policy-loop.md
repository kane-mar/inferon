## ITEM-064: Adaptive policy loop (telemetry → analysis → versioned policy → hot path)

**As the** operator
**I want** the running system to learn from its own telemetry and adapt routing,
budgets, and thresholds without a redeploy
**So that** operation improves continuously and a bad adaptation is visible and
reversible

Today's heuristics are **static and open-loop**: the router threshold (0.35), the
cheap fallback model, the provider order, and the alert thresholds are set by hand
and never learn. The observation data now exists (cost, latency, cache, quality,
provider health), so a closed loop is possible.

**Acceptance Criteria:**
1. A scheduled **analysis job** (SQL + the existing embedding path) derives a
   per-tenant **policy snapshot**: routing threshold, provider weights, per
   task-cluster model preference, spend forecast.
2. The policy is **versioned, auditable, and reversible** — each change records
   its evidence (the metrics that justified it) and can be rolled back.
3. The hot path reads the current policy the same way it already reads
   budget/router config — **applied on the next request, no redeploy**.
4. **Guardrails**: changes are bounded (max delta per step), a frontier fallback
   is always retained, exploration/holdout is capped, and a **kill switch**
   freezes to the last-known-good policy.
5. Adaptation is **measured**: policy version ↔ outcome (quality/cost/latency),
   so a bad policy is detected and rolled back.

**Technical Notes:**
- Offline batch is sufficient at this scale; no streaming ML. The loop is:
  `traces → analysis → policy row → per-request read`.
- Composes existing/planned items: ITEM-029 (task clusters), ITEM-030 (Lagrangian
  budget solver), ITEM-055 (quality), ITEM-058 (savings), ITEM-061 (provider
  health), ITEM-063 (cache affinity). This item is the **loop and its governance**,
  not a re-implementation of those.
- Exploration must never silently degrade a paying tenant: challengers run on a
  small held-out share and are gated by the ITEM-055 quality signal.
- Keep the policy in Postgres with a `policy_version` on traces so outcomes are
  attributable (extend the trace, like `router_event`).

**Source:** 2026-10-12 adaptive-operation analysis — apply intelligence to the
running operation by closing the observation → policy → hot-path loop.
