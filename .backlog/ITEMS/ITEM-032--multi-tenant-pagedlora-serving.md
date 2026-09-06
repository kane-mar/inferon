## ITEM-032: OmniRouting — Multi-Tenant PagedLoRA Serving (Slice 5 · was 022e) — STAGED

**As a** product owner
**I want** a shared GPU pool that pages tenant LoRAs in and out of VRAM on demand
**So that** per-tenant adapter serving doesn't mean per-tenant GPU waste

**Acceptance Criteria (staged — not to be pulled while Slice 3 is parked):**
1. A shared GPU pool hosts base models in VRAM with tenant LoRAs paged in/out of host RAM on demand (PagedLoRA pattern — see `wiki/llm-inference/multi-lora-serving.md`)
2. Adapter hot-swap and routing happen without downtime and without breaking the client OpenAI schema
3. Pool economics are observable (GPU utilization, adapter residency, swap cost) in operator views

**Technical Notes:**
- Parent: ITEM-022 (archived), slice 022e
- **Gate: owning a GPU serving fleet** (Slice 3+). Like ITEM-031, visible and rankable but not pullable today
- Revisit only when hosted-only routing (ITEM-028..030) is proven insufficient or Slice 3 unparking is on the table

**Decisions (PO, 2026-09-05):**
- **Unpark trigger = backlog order** (same as ITEM-031): activates when it reaches the top; no separate trigger criteria.
- **GPU economics err toward the most performant model.** When the GPU tier activates (base-model pinning, engine choice, persistent-vs-serverless topology), pick the option that runs the most performant model for the workload; cost is a secondary consideration, not the deciding one. Implication: the spec's persistent 4× L4 node with a pinned best-in-class base is the default direction unless it materially underperforms.
