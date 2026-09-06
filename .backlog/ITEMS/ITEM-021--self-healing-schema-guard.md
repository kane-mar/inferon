## ITEM-021: Self-Healing Schema Guard (Slice 4)

**As a** developer running cheaper/smaller models in production
**I want** Inferon to guarantee my responses are schema-valid even when the cheap model drifts
**So that** I get the cost of a small model with none of the production risk

**Acceptance Criteria:**
1. A streaming output verifier validates output tokens against the expected JSON schema as they arrive (syntax first, then field constraints)
2. On invalid syntax, hallucinated fields, or an upstream error, the original prompt is instantly rerouted to a frontier model in the background — the end user never sees an error or a malformed response
3. The fallback is transparent mid-stream: no client-visible break, no duplicated side effects from the failed attempt
4. Prompts that triggered a fallback are tagged as negative samples in the traces table (harvested for the next distillation cycle)
5. Fallback + verification add no measurable TTFT regression on the happy path

**Technical Notes:**
- Roadmap source: Slice 4 in `wiki/llm-inference/inferon-product-roadmap.md` ("Streaming Output Verifier", "Transparent Fallback Circuit", "Negative Sample Harvester")
- **Dependency flag for refinement**: the roadmap frames this as guarding *distilled-model* traffic (Slice 3, ranked last). When this item reaches the top, decide: guard hosted small-model traffic (deepseek-v4-flash etc. via the Slice 2 router) first, or pull Slice 3 up. The schema source ("user's expected schema") also needs a decision — `response_format` passthrough vs. inferred
- Large item — left chunky per progressive granularity; break down when it reaches the top of the backlog
