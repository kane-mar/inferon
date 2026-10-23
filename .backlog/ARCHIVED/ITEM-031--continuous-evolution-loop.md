## ITEM-031: OmniRouting — Continuous Evolution Loop (Slice 5 · was 022d) — SLICE 1 DELIVERED

**As a** product owner
**I want** drift in an adapter to trigger retraining and a hot upgrade by itself
**So that** the system gets cheaper, faster, and more specialized without anyone touching it

**Acceptance Criteria:**
1. A background workflow (spec: Temporal.io) continuously watches per-LoRA fallback rate from ITEM-026's guard telemetry
2. Fallback rate > 5% on an adapter (concept drift) queues a new training job on the harvested negative samples
3. The new adapter passes a held-out gold-set test before being issued via `load_lora_adapter` for a seamless endpoint upgrade
4. Every step (drift detected → trained → tested → swapped) is observable in operator views

**Technical Notes:**
- Parent: ITEM-022 (archived), slice 022d
- **Gate: Slice 3 distillation infra** (S3 adapter weights, vLLM cluster, DPO dataset shape from ITEM-026). **Unparked and promoted to highest priority by the PO on 2026-10-15**; this gate is still real, so the first action is to refine this into a pullable first slice and identify which gate pieces are actually available.
- The trigger is backlog order (below): being at the top unparks it.

**Decisions (PO, 2026-09-05):**
- **Unpark trigger = backlog order.** No special data trigger: Slice 3/distillation work is ranked below the actionable backlog, and "when we get to it" means when this item shuffles to the top it is, by definition, unparked and gets pulled. Staging is informational only — the single-ranked list is the trigger mechanism.

## Close-out — slice 1 (2026-10-15)

The ACs that need the Slice 3 infra (train, held-out gold set, hot swap) are
carried by **ITEM-076**. This item's pullable half is delivered:

- **AC1 (drift detection)** — `evolution::detect_drift` aggregates the guard
  telemetry per `guarded_model`: `count(*)` and
  `count(*) FILTER (outcome in (fallback, fallback_failed))` over 24h, sample
  floor 20; `DriftSignal::is_drifted` = fallback rate > 5%.
- **AC4 (observability, for the steps that exist)** — `evolution_jobs`
  (migration 034) records drift-detected → queued with evidence; the operator
  page lists adapter, state, fallback rate and detection time, and
  `GET /operator/evolution` serves the same.
- **Dedupe** — a partial unique index allows one *open* job per
  (tenant, adapter); a repeat pass returns `queued: 0`.
- **Scheduling** — `run_evolution` every 15 minutes.
- **Live**: 25/25 fallbacks on `deepseek-flash` queued exactly one job with
  `{requests:25, fallbacks:25, rate:1.0}`; a 19-response model sat below the
  floor and was not queued; a second pass deduped; the rendered operator page
  showed the row.
- Rust **308 → 310** tests, clippy clean. Commits `3545a38`, `adf8c83`,
  `5f32610`; CI `39898843659`, `39901625513`, `39906587927`.
- **Live-caught bug**: the operator list joined `tenants.name`, which does not
  exist — the query failed and `unwrap_or_default()` hid it as an empty list.
  Fixed (`adf8c83`) and the error is now surfaced instead of swallowed.
