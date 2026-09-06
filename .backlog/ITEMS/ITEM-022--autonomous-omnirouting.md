## ITEM-022: Autonomous OmniRouting (Slice 5)

**As a** product owner
**I want** the router to optimize cost/quality continuously and without human tuning
**So that** every tenant's traffic gets progressively cheaper, faster, and more specialized each week

**Acceptance Criteria:**
1. A semantic embedding classifier on the ingress path matches each prompt to the best task/adapter in <3ms overhead
2. A Lagrangian budget solver sets the quality-gap threshold (epsilon) dynamically from remaining monthly budget, p95 latency targets, and confidence — replacing the static Slice 2 classifier threshold
3. Multi-tenant LoRA serving pages adapters in/out of VRAM on demand (no per-tenant GPU waste)
4. A continuous evolution loop detects production prompt drift, schedules retraining, and canaries new adapters into the live pool with zero downtime
5. The loop is observable end-to-end (what routed where, why, at what cost/quality) in the operator views

**Technical Notes:**
- Roadmap source: Slice 5 in `wiki/llm-inference/inferon-product-roadmap.md` ("Semantic Embedding Classifier", "Lagrangian Multiplier Solver", "Multi-Tenant PagedLoRA Serving", "Continuous Evolution Loop")
- **Depends on the distillation + schema-guard machinery** (Slice 3 / ITEM-021): the adapter pool and the verified-fallback net are prerequisites for safe autonomous routing. Expect major re-ranking at refinement — this sits at the bottom of the backlog and will likely split as the slices above it land
- Largest remaining item — deliberately chunky; break down only when it approaches the top
