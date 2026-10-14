## ITEM-071: Per-provider degradation alerting (ITEM-068 AC5)

**As a** tenant with more than one provider
**I want** to be alerted when a *specific* provider degrades
**So that** a cheap-but-failing or slow provider is demoted/investigated before
it hurts

Split from ITEM-068 (2026-10-14): tenant-level error-rate and latency-regression
alerting is delivered. This adds the per-provider axis, which needs per-entity
signals rather than the tenant's aggregate.

**Acceptance Criteria:**
1. Per-provider error rate and p95 are computed from `traces.provider_id` over
   the trailing day (vs the preceding day for latency), with a minimum-sample
   guard.
2. A degraded provider fires a **per-entity class** — `provider_error:<id>` /
   `provider_latency:<id>` — through the ITEM-020 lifecycle, so each provider has
   its own episode and clear.
3. The operator alert view labels the provider (its name, not just the uuid).
4. Thresholds reuse the tenant's `alert_error_rate` / `alert_latency_multiplier`
   (per-provider overrides are out of scope).

**Technical Notes:**
- Mirror `limit_breaches`/`evaluate_limits` (ITEM-047): aggregate per provider,
  fire/clear one episode per entity.
- `providers` reachability (`last_check_ok`) is a probe, not a health signal —
  do not conflate "reachable" with "healthy".

**Source:** ITEM-061 AC3 → ITEM-068 → split 2026-10-14.
