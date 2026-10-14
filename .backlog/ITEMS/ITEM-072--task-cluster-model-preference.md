## ITEM-072: Task-cluster model preference (ITEM-069 AC2)

**As a** tenant
**I want** the policy to learn "this cheap model is good enough for task X"
**So that** routing is driven by measured adequacy per task, not just complexity

Split from ITEM-069 (2026-10-14): provider health weights are delivered. This is
the other half of the policy's *content*, and it depends on the staged
OmniRouting work.

**Acceptance Criteria:**
1. Task clusters (ITEM-029 centroids, from `system_prompt_hash` / embeddings) are
   mapped to their measured quality per model (ITEM-055 outcomes).
2. The policy records a **per-cluster model preference** (e.g. "cluster X: the
   cheap tier is adequate") and the router consults it, so a cheap model is
   chosen only where it has been shown good enough.
3. Inherits ITEM-064/069 guardrails (bounded, holdout-gated, kill switch,
   rollback) and stays measurable via `traces.policy_version`.

**Technical Notes:**
- Depends on staged ITEM-029 (task centroids) and ITEM-030 (Lagrangian solver);
  this is the policy surface + hot-path application, not the clustering.
- Keep the policy JSONB additive: an old hot path ignores the new field.

**Source:** ITEM-069 AC2 → split 2026-10-14.
