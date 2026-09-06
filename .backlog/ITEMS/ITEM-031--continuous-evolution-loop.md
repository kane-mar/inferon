## ITEM-031: OmniRouting — Continuous Evolution Loop (Slice 5 · was 022d) — STAGED

**As a** product owner
**I want** drift in an adapter to trigger retraining and a hot upgrade by itself
**So that** the system gets cheaper, faster, and more specialized without anyone touching it

**Acceptance Criteria (staged — not to be pulled while Slice 3 is parked):**
1. A background workflow (spec: Temporal.io) continuously watches per-LoRA fallback rate from ITEM-026's guard telemetry
2. Fallback rate > 5% on an adapter (concept drift) queues a new training job on the harvested negative samples
3. The new adapter passes a held-out gold-set test before being issued via `load_lora_adapter` for a seamless endpoint upgrade
4. Every step (drift detected → trained → tested → swapped) is observable in operator views

**Technical Notes:**
- Parent: ITEM-022 (archived), slice 022d
- **Gate: Slice 3 distillation infra** (S3 adapter weights, vLLM cluster, DPO dataset shape from ITEM-026). This row exists so the work is visible and rankable; it is not pullable today
- Slice 3 is deliberately parked by the PO — revisit this gate when/if that decision changes
