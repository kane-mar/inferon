## ITEM-022: Autonomous OmniRouting (Slice 5)

**As a** product owner
**I want** the router to optimize cost/quality continuously and without human tuning
**So that** every tenant's traffic gets progressively cheaper, faster, and more specialized each week

**Acceptance Criteria (refined 2026-09-05):**
1. An in-process embedding classifier assigns each prompt to a task centroid in <3ms added latency, as a signal layered over the ITEM-015 heuristics (not a replacement of them)
2. A Lagrangian budget solver sets the quality-gap threshold (epsilon) dynamically from remaining budget and days-in-month — replacing the static ITEM-015 threshold — and the choice is observable per request in the trace
3. When budget is healthy the router favors the frontier tier (epsilon tight); as budget depletes it loosens epsilon toward the cheap tier to avoid the ITEM-016 hard 429 cutoff — spend math stays integer-cents on the Postgres flush path
4. Cheap/frontier tier assignment keeps the existing byte-identical passthrough guarantee for non-routed requests and never breaks the client OpenAI schema
5. Drift-triggered retraining + hot adapter swap and multi-tenant paged LoRA serving are specified and staged, not built, until the distillation slice lands

**Refinement decisions (2026-09-05):**
- **Split into a now-able hosted-model core and Slice-3-gated extensions.** The "Inference Economics Engine" (embed → task match → epsilon from budget burn) routes between *hosted* frontier vs cheap tiers and needs no distillation. Only the adapter-evolution half (retrain/swap LoRAs, PagedLoRA pool) requires Slice 3's vLLM/S3 machinery. Like ITEM-021, this resolves the original capture's dependency flag: the core is orderable above Slice 3.
- **Epsilon schedule** comes from the spec verbatim (`lagrangian_solver.rs`): `burn_ratio = budget_remaining/days_left ÷ (monthly_budget/30)`; >1.2 → eps 0.02 (favor quality), <0.5 → eps 0.15 (aggressive cheap routing), else 0.05. It consumes the ITEM-016 `current_spend_cents` + `budget_limit_cents` the breaker already maintains — no new counter.
- **Embedding classifier is additive**: the ITEM-015 complexity classifier stays the fast default; centroids are bootstrapped from existing per-prompt-hash traces (task clusters), not hand-labeled.

## Breakdown (2026-09-05 — sprintable slices, in pull order)

- **022a FastEmbedder** — `ort` (ONNX Runtime) crate; quantized BGE-micro-class model; (1,128) token ids → 384-d normalized vector; <3ms budget. AC: embed latency measured on the hot path; model artifact bundled/stored by a documented path (image size + download decision); pure-Rust, no Python at runtime (project decision).
- **022b Task-centroid matching** — build/refresh centroids from trace clusters (by system_prompt_hash + linter class); cosine match prompt→task; emit the task assignment into the routing decision + trace. AC: classification quality gate vs the ITEM-015 heuristic on the same corpus; graceful no-match fallback to the heuristic path.
- **022c Lagrangian budget solver** — implement the burn-ratio epsilon schedule; wire it as the dynamic threshold replacing ITEM-015's static one; route to cheap tier when predicted gap ≤ eps; fully observable (eps value + rationale on the trace; operator view). AC: unit tests over the schedule boundaries (TDD — healthy/balanced/depleted budget); end-to-end: high-spend month-end actually loosens routing before the 429 cap (ties to ITEM-016's 15% zone).
- **022d Continuous evolution loop** — *staged, not built*: Temporal.io workflow watching per-LoRA fallback rate (>5% → drift), queue retrain on harvested negatives, held-out gold-set test, `load_lora_adapter` hot-swap. Gate: Slice 3 distillation infra (S3 weights, vLLM cluster, DPO dataset shape from ITEM-021d).
- **022e Multi-tenant PagedLoRA serving** — *staged, not built*: shared GPU pool paging tenant LoRAs in/out of VRAM. Gate: owning a GPU serving fleet (Slice 3+); revisit only when hosted-only routing is proven insufficient.

**Gates / open decisions (PO):**
1. Embedding model artifact: size vs quality (BGE-micro ~30MB class) and where it ships (image layer vs first-boot download) — affects the ~25GB snapshot/backup story
2. Whether 022b/c are worth building before real traffic (ITEM-018/020) provides centroids and burn patterns — refinement may re-rank this below 020's alerting once 018 lands
3. When the hosted-only core is live, revisit the Slice 3 parking decision with real fallback-rate + drift data from 021

**Technical Notes:**
- Source: `wiki/llm-inference/inferon-slice-engineering-spec.md` Slice 5 + roadmap Slice 5; solver formulas and embedder params from the spec's code-level details
- The spec assumes gpt-4o/claude as frontier; in the DeepSeek-only world "frontier vs cheap" maps to the tenant's original requested model vs the downgrade target — consistent with the ITEM-021 fallback decision
- Biggest remaining item in the backlog; deliberately chunky until it approaches the top
