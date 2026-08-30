# Budget-Constrained Routing (OmniRouter)

> Sources: User-provided technical specification, 2026-08-30
> Raw: [aio-proxy-technical-specification.md](../../raw/llm-inference/aio-proxy-technical-specification.md); [inferon-vertical-slice-roadmap.md](../../raw/llm-inference/inferon-vertical-slice-roadmap.md); [inferon-slice-engineering-spec.md](../../raw/llm-inference/inferon-slice-engineering-spec.md); [2026-08-30-inferon-project-direction.md](../../raw/llm-inference/2026-08-30-inferon-project-direction.md); [2026-08-30-inferon-budget-counter-decision.md](../../raw/llm-inference/2026-08-30-inferon-budget-counter-decision.md)
> Updated: 2026-08-30

## Overview

OmniRouter is the budget-constrained optimization core of the AIO-Proxy data plane. For every request it predicts the quality gap between a frontier model and a locally served distilled adapter, then makes a Lagrangian-optimized routing decision under token-budget and latency constraints.

## Decision Pipeline

1. **Complexity Classifier** — embeds the prompt via a fast bi-encoder (<3ms). Featurization also extracts structural metadata: system prompt hash, message count, tool definitions, token length. The embedder is a quantized bi-encoder (e.g., BGE-micro / ModernBERT-base running on ONNX runtime on CPU).
2. **Dual-Head Scoring** — predicts the Quality Gap dq = Qhat_frontier(x) - Qhat_distilled(x).
3. **Lagrangian Solver** — evaluates real-time token budget vs. latency SLA and produces a dynamic acceptance threshold.

## Optimization Formulation

The router solves a constrained knapsack problem for each query x:

```
max_{m in M} E[Q(m, x)]   subject to   sum_{i=1..N} Cost(m_i, x_i) <= B_period,   Latency(m, x) <= L_max
```

- If dq <= epsilon_threshold, dispatch to the local distilled model; otherwise route to the frontier model.
- epsilon is **dynamically adjusted** by the remaining budget B_period via Lagrangian multipliers: as budget depletes, epsilon_threshold shifts so higher-complexity queries are aggressively routed to the small model — this is the mechanism that prevents hard service cutoffs under budget pressure.

## Routing Decision (Rust Dataflow)

The decision function (`determine_route`) takes a `RoutingDecisionContext` (tenant_id, prompt_tokens, system_prompt_hash, embedding_vector, remaining_budget_usd) and:

1. Checks the adapter registry for an active, validated LoRA for the tenant + system prompt hash (the task cluster key).
2. Predicts the quality gap on the specific prompt vector via `router.predict_quality_gap`.
3. Evaluates the dynamic Lagrangian threshold from remaining budget via `router.get_current_epsilon_threshold(remaining_budget_usd)`.
4. Routes to `DistilledLoRA { base_model_endpoint, adapter_id }` when the predicted gap is within threshold; otherwise falls back to `FrontierProvider` — default Anthropic `claude-3-5-sonnet-20241022`.

Routing is a sub-5ms evaluation of cost, latency, and expected quality gap in the full end-to-end flow.

## Tenant Configuration Knobs

From `tenant_configurations` (PostgreSQL):

- `routing_strategy` — `max_quality`, `balanced_cost` (default), or `hard_budget`.
- `min_quality_threshold` — default 0.95.
- `max_p95_latency_ms` — default 2000.
- `monthly_budget_usd` — default 500.00; the router reads `remaining_budget_usd` (derived from `current_spend_usd`) at decision time.
- `frontier_fallback_model` — default `claude-3-5-sonnet-20241022`.

> **Status: Outdated** (2026-08-30)
> The [project direction](inferon-project-direction.md) sets the operational frontier/default model to **DeepSeek**; the spec's claude-3-5-sonnet-20241022 default remains source-documented but is superseded as the live default. DeepSeek pricing is not yet specified in any source, so the cost table (gpt-4o $2.50/$10.00, gpt-4o-mini $0.15/$0.60, default $3.00/$15.00 per 1M tokens) needs a DeepSeek row when building.

## Cold Start Behavior

When a customer introduces a completely new prompt template (no adapter exists for the task cluster), 100% of traffic routes to the frontier model initially while the dataset plane clusters traces and queues the initial distillation job. See [passive-lora-distillation](passive-lora-distillation.md).

## Roadmap Evolution (Inferon)

The [Inferon Product Roadmap](inferon-product-roadmap.md) stages this subsystem into two slices:

- **Slice 2 (Days 6-10) — Heuristic Model Router & Budget Circuit Breaker**: before the embedding classifier arrives, routing runs on a sub-1ms heuristic rule engine checking prompt token length, tool definitions, and system prompt tags. A budget state engine keeps in-memory Redis/Atomic counters of tenant spend in real time; approaching a monthly limit (e.g., $500) automatically enforces graceful downgrades or alerts (the hard-budget circuit breaker). A dynamic upstream client multiplexes requests across provider APIs without breaking the client's OpenAI SDK schema. Routing targets for simple tasks include cheaper hosted models gpt-4o-mini and claude-3-haiku, while heavy reasoning stays on frontier models.
- **Slice 5 (Days 25-30) — Fully Autonomous OmniRouting**: the semantic embedding classifier embeds prompts via a quantized ONNX model (BGE-micro or ModernBERT) in <3ms to match tasks to active LoRA adapters. The Lagrangian multiplier solver computes epsilon_threshold from three inputs: remaining monthly budget, p95 latency targets, and model confidence scores (confidence scores are an additional input beyond the spec's budget + latency formulation).

## Engineering Implementation (Detailed Spec)

Concrete mechanics from the [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md):

- **Slice 2 interception**: the proxy reads the incoming JSON payload into a serde_json::Value struct and scores complexity with three heuristics — presence of tools or functions arrays, temperature > 0.8 (creative generation vs strict extraction), and token length < 500. On a low score it rewrites payload["model"] = "gpt-4o-mini" (or Anthropic claude-3-haiku), updates the upstream destination URL, and forwards.
- **Budget breaker enforcement**: an atomic spend counter via `HINCRBY tenant:{id}:spend {cost_in_cents}` in Redis; on exceeding the monthly limit the proxy either rejects with HTTP 429 or forcefully overrides the model parameter to the cheapest available open-source model.
- **Slice 5 classification**: the ort (ONNX Runtime) crate loads a highly quantized embedding model (like BGE-micro); prompts vectorize in <3ms and Cosine Similarity against known task centroids in memory assigns the workload to a LoRA adapter.
- **Lagrangian semantics**: solves `max_{m in M} E[Q(m, x)] subject to sum_{i=1..N} Cost(m_i, x_i) <= B_remaining`. When B_remaining is high early in the month, epsilon tightens and more traffic routes to the frontier model to maximize quality; as budget depletes, epsilon loosens and acceptable traffic routes to the zero-cost distilled adapters to prevent a hard cutoff.

Code-level specifics (from the [complete implementation spec](inferon-slice-engineering-spec.md)):

- **Cost table (per 1M tokens)**: gpt-4o $2.50 prompt / $10.00 completion; gpt-4o-mini $0.15 / $0.60; default fallback $3.00 / $15.00.
- **ComplexityClassifier score** (0.0-1.0): tools array non-empty +0.50; response_format type json_object +0.30; content > 8000 chars +0.40 or > 3000 chars +0.20; code regex (```/def /class /fn /import /SELECT /FROM ) +0.30; JSON-spec regex ("type": "object" / "properties") +0.25; capped at 1.0. Downgrade when score < 0.35 and model is gpt-4o.
- **BudgetCircuitBreaker states**: Allowed; EnforceCheapFallback within 15% of budget depletion (current_spend + budget_limit_usd * 0.15 >= budget_limit_usd); ExceededHardCap at current_spend >= budget_limit_usd.
- **Epsilon values (LagrangianRouter)**: burn_ratio = expected_daily_burn_allowed / (monthly_budget_usd / 30.0); > 1.2 → epsilon 0.02 (strict); < 0.5 → epsilon 0.15 (aggressive small-model routing); otherwise 0.05 (balanced).
- **FastEmbedder**: ort SessionBuilder with GraphOptimizationLevel::Level3 and intra_threads 4; 384-d normalized vector from (1, 128) input.

> **Status: Outdated** (2026-08-30)
> The [project direction](inferon-project-direction.md) resolves this: the spend counter lives in PostgreSQL (atomic UPDATE on tenant_configurations.current_spend_usd) for the MVP, in integer cents; Redis fast-path + async reconciliation is the documented scale-out path. Both Redis counter variants (HINCRBY tenant:{id}:spend {cost_in_cents}, tenant:{id}:monthly_spend float USD) are superseded for the MVP.

## See Also

- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
- [Passive LoRA Distillation](passive-lora-distillation.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
- [Inferon Project Direction](inferon-project-direction.md)
