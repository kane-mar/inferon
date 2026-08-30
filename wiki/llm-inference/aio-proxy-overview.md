# AIO-Proxy: Autonomous Inference Optimizer

> Sources: User-provided technical specification, 2026-08-30
> Raw: [aio-proxy-technical-specification.md](../../raw/llm-inference/aio-proxy-technical-specification.md); [inferon-vertical-slice-roadmap.md](../../raw/llm-inference/inferon-vertical-slice-roadmap.md); [inferon-slice-engineering-spec.md](../../raw/llm-inference/inferon-slice-engineering-spec.md); [inferon-complete-implementation-spec.md](../../raw/llm-inference/inferon-complete-implementation-spec.md); [2026-08-30-inferon-project-direction.md](../../raw/llm-inference/2026-08-30-inferon-project-direction.md)
> Updated: 2026-08-30

## Overview

AIO-Proxy is a zero-SDK, drop-in proxy (client changes baseURL only) that combines **Automated Passive LoRA Distillation** with **Global Budget-Constrained Multi-Model Routing**. It transparently replaces the OpenAI-compatible endpoint, routing each request to either a frontier provider or a locally served, auto-distilled LoRA adapter based on predicted quality gap and remaining budget. The product roadmap names this system **Inferon** at https://api.inferon.dev/v1 (see [Inferon Product Roadmap](inferon-product-roadmap.md)); the spec uses the working name AIO-Proxy at https://api.aioproxy.com/v1. Per the [project direction](inferon-project-direction.md) (2026-08-30), the working name is **inferon**; AIO-Proxy is retired.

## Three Asynchronous Planes

The system is split into three planes with strictly separated latency profiles:

1. **Data Plane (Hot Path)** — Sub-15ms proxy routing requests between frontier providers, self-hosted base models, and auto-distilled LoRA adapters.
2. **Telemetry & Dataset Plane (Warm Path)** — Asynchronous log ingestion, heuristic scoring, synthetic validation pair generation, and dataset clustering.
3. **Training & Distillation Plane (Cold Path)** — Autonomous fine-tuning pipelines (LoRA/QLoRA on small open-weight models), offline benchmarking, and zero-downtime adapter hot-swapping.

## Data Plane (Hot Path)

The hot path is a five-stage pipeline:

1. **Zero-Friction Proxy Ingress** — Rust/Axum; auth & tenant context extraction (API keys, organization ID), schema validation, token counting.
2. **Budget-Constrained Router (OmniRouter)** — complexity classifier embeds the prompt via a fast bi-encoder (<3ms); dual-head scoring predicts the quality gap d(q_frontier - q_distilled); a Lagrangian solver evaluates real-time token budget vs. latency SLA. See [budget-constrained-routing](budget-constrained-routing.md).
3. **Distilled Model Cluster** — vLLM/SGLang engine cluster with dynamic multi-LoRA serving pool over quantized 1B-8B base engines. See [multi-lora-serving](multi-lora-serving.md).
4. **Frontier Upstream Providers** — Anthropic (Claude 3.5 Sonnet), OpenAI (GPT-4o), Google (Gemini Pro).
5. **Response Streaming & Shadow Sampling Hook** — streams tokens back to the client instantly; asynchronously pushes a payload trace to a Kafka / Redpanda buffer.

> **Status: Outdated** (2026-08-30)
> The [project direction](inferon-project-direction.md) selects PostgreSQL as the telemetry/control-plane database, superseding the Kafka/Redpanda transport in the original AIO-Proxy spec and narrowing the roadmap's "SQLite/PostgreSQL" to PostgreSQL only. Telemetry is a Tokio channel -> PostgreSQL traces table.

Low-complexity requests route to the distilled cluster; high-complexity requests route to frontier providers.

## Telemetry & Distillation Control Plane (Warm & Cold Paths)

Downstream of the hot path, an asynchronous trace stream feeds:

- **Trace Sanitation & Quality Filter** — filters on 200 OK and regex checks; de-duplicates via MinHash LSH; synthetic edge-case generation.
- **Automated LoRA Training Pipeline** — auto-clusters prompts by task; triggers unsloth/Axolotl runs; evaluates against a Frontier Gold Set. See [passive-lora-distillation](passive-lora-distillation.md).
- **Adapter Registry & Hot Deployment** — uploads weights to S3/MinIO; registers new LoRAs with vLLM; updates the router cost/quality graph.

## Control Plane Data Schema (PostgreSQL)

Two tables. **tenant_configurations** carries per-tenant routing policy: `monthly_budget_usd` (default 500.00), `current_spend_usd` (default 0.00), `routing_strategy` (default `balanced_cost`; options `max_quality`, `balanced_cost`, `hard_budget`), `min_quality_threshold` (default 0.95), `max_p95_latency_ms` (default 2000), and `frontier_fallback_model` (default `claude-3-5-sonnet-20241022`).

**distilled_adapters** records adapter artifacts: `task_signature` (SHA256 of system prompt + tool schemas), `base_model` (e.g., `meta-llama/Llama-3.2-3B-Instruct`), `adapter_s3_uri`, `sample_count`, `benchmark_parity_score` (e.g., 0.985), `is_active` (default FALSE), and `deployed_at`.

The implementation-level schema (from the [complete implementation spec](inferon-slice-engineering-spec.md)) is `tenants` + `traces`: `tenants` stores api_key_hash plus per-tenant master upstream keys (upstream_openai_key, upstream_anthropic_key); `traces` records request_model, system_prompt_hash, prompt_tokens, completion_tokens, time_to_first_token_ms, total_duration_ms, upstream_cost_usd NUMERIC(10, 6), waste_detected JSONB, status_code, raw_prompt, raw_completion — indexed on (tenant_id, created_at DESC) and system_prompt_hash.

## Implementation Latency & Waste Rules

- **Per-slice hot-path latency** (from the implementation matrix): Slice 1 <1ms (zero-copy stream), Slice 2 <2ms (regex & payload AST), Slice 3 <2ms (routing to vLLM), Slice 4 <5ms (buffered schema check), Slice 5 <3ms (in-process ONNX embed).
- **Waste-detection rule**: a gpt-4o request with prompt_tokens < 100 and completion_tokens < 50 is flagged over_provisioned with suggested_model gpt-4o-mini.
- **Cost table (per 1M tokens)**: gpt-4o $2.50 / $10.00, gpt-4o-mini $0.15 / $0.60, default $3.00 / $15.00 (prompt / completion).

## End-to-End Execution Sequence

1. **Ingress & Traffic Interception** — the client changes baseURL to https://api.aioproxy.com/v1 in their OpenAI/Anthropic SDK; every standard request passes through the Axum proxy.
2. **Dynamic Decision & Routing** — sub-5ms evaluation of cost, latency, and expected quality gap. The proxy checks the system prompt hash against the tenant's adapter registry; if an adapter exists it runs a lightweight complexity regression. Requests within quality tolerance route to the internal vLLM cluster with the specific adapter_id; complex or novel edge cases pass to the configured frontier API.
3. **Response Streaming & Telemetry Logging** — asynchronous trace capture with zero added latency. The upstream token stream pipes directly to the client via SSE; concurrently a complete event record (input payload, system prompt, output tokens, time-to-first-token, completion status) is pushed to the Redpanda trace queue.
4. **Background Distillation Pipeline** — the background worker pulls traces from the queue, clusters them by semantic schema, and checks whether a task cluster crossed the training threshold (N >= 1,000 samples). An ephemeral worker runs LoRA distillation on an open-weight base model, benchmarks against frontier responses, and outputs an adapter artifact.
5. **Hot-Swap & Continuous Optimization** — the newly minted LoRA registers in the vLLM multi-LoRA pool; the proxy updates its routing table and begins routing a small percentage (canary release) of matching traffic to the new adapter, gradually increasing share as real-time quality verification scores stay high.

## Failure Modes & Mitigations

| Failure Mode | Root Cause | Mitigation |
|--------------|------------|------------|
| Model Drift / Schema Corruption | Distilled model hallucinates on novel inputs | Real-time output schema validation; on JSON parse failure, re-run against the frontier model and downweight the LoRA routing probability |
| Budget Depletion Early in Billing Cycle | Sudden traffic spike | Lagrangian multiplier shifts epsilon_threshold, aggressively routing higher-complexity queries to the small model to prevent hard service cutoffs |
| Cold Start on New Workload Types | Customer introduces a completely new prompt template | 100% of traffic routes to the frontier model initially while the dataset plane clusters traces and queues the initial distillation job |
| vLLM GPU OOM | Too many concurrent active LoRA adapters | PagedLoRA memory manager unloads inactive adapters from VRAM to Host DRAM via LRU eviction |

## MVP Rollout Checklist

- **Week 1-2 (Proxy Core)** — Rust/Axum proxy with transparent OpenAI/Anthropic streaming at zero latency penalty; stream all completions to Redpanda/PostgreSQL.
- **Week 3-4 (Distillation Pipeline)** — automated dataset formatting and Unsloth training script on an on-demand GPU instance.
- **Week 5-6 (Multi-LoRA Serving)** — a single 4x L4 GPU node running vLLM with dynamic multi-LoRA routing.
- **Week 7-8 (OmniRouter Logic)** — prompt embedding classification and budget-constrained routing policies in the proxy core; developer console showing cost savings.

## See Also

- [Budget-Constrained Routing](budget-constrained-routing.md)
- [Passive LoRA Distillation](passive-lora-distillation.md)
- [Multi-LoRA Serving](multi-lora-serving.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
- [Inferon Project Direction](inferon-project-direction.md)
