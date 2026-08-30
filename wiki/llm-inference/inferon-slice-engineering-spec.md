# Inferon Slice Engineering Specification

> Sources: User-provided engineering specification, 2026-08-30; complete implementation specification, 2026-08-30
> Raw: [inferon-slice-engineering-spec.md](../../raw/llm-inference/inferon-slice-engineering-spec.md); [inferon-vertical-slice-roadmap.md](../../raw/llm-inference/inferon-vertical-slice-roadmap.md); [inferon-complete-implementation-spec.md](../../raw/llm-inference/inferon-complete-implementation-spec.md); [2026-08-30-inferon-budget-counter-decision.md](../../raw/llm-inference/2026-08-30-inferon-budget-counter-decision.md)
> Updated: 2026-08-30

## Overview

The implementation-level companion to the [Inferon Product Roadmap](inferon-product-roadmap.md): a detailed execution architecture for each of the 5 vertical slices, covering the Rust proxy internals (axum/reqwest, tokio channels, serde_json mutation, ort/ONNX embedding), the Python/Modal training pipeline (Unsloth QLoRA), vLLM dynamic serving, the schema-validation safety net, and the Temporal-driven autonomous evolution loop.

## Slice 1 — Ingress, Telemetry & Cost Linter Implementation

- **Zero-copy stream forwarding**: the proxy must add <1ms of latency to upstream API calls. LLM completions use SSE; to avoid buffering the response in memory (which breaks TTFT—Time-To-First-Token), the proxy maps the incoming Axum Body into a reqwest::Body::wrap_stream and maps upstream bytes back into the Axum response as text/event-stream.
- **Authentication**: extracts the `Authorization: Bearer <API_KEY>` header, validates it against a local cache (or Redis), and swaps it with Inferon's master upstream API key for OpenAI/Anthropic.
- **Async telemetry**: on request completion, a lightweight metadata struct (Tenant ID, upstream URL, token counts from the final [DONE] chunk, total duration) goes through a tokio::sync::mpsc channel; a dedicated Tokio background worker does batch inserts into a PostgreSQL traces table every 500ms, so database I/O never blocks the hot HTTP path.
- **Cost linter engine**: the background worker runs heuristic checks on saved payloads — e.g., repeating 4,000+ token system prompts without provider cache headers, or formatting simple unstructured text using gpt-4o instead of gpt-4o-mini.

Code-level specifics (from the complete implementation spec):

- **Stack**: axum 0.7, tokio 1.38, reqwest 0.12 (stream/json), tower 0.4, tower-http 0.5, sqlx 0.7 (runtime-tokio/postgres/uuid/chrono/json), uuid 1.8, redis 0.25, regex 1.10, rust_decimal 1.35.
- **Telemetry channel**: tokio mpsc channel(10_000); the batch worker flushes every 500ms or every 100 items, whichever comes first; TraceMessage carries tenant_id, request_model, system_prompt_hash, token counts, ttft_ms, total_duration_ms, status_code, raw_prompt, raw_completion.
- **DB schema (schema.sql)**: `tenants` (id, api_key_hash, upstream_openai_key, upstream_anthropic_key — master upstream keys are stored per tenant); `traces` (request_model, system_prompt_hash, prompt_tokens, completion_tokens, time_to_first_token_ms, total_duration_ms, upstream_cost_usd NUMERIC(10, 6), waste_detected JSONB, status_code, raw_prompt, raw_completion); indexes on (tenant_id, created_at DESC) and system_prompt_hash.
- **Waste-detection linter rule**: gpt-4o request with prompt_tokens < 100 and completion_tokens < 50 → waste_detected.over_provisioned = true with suggested_model gpt-4o-mini.
- **Cost table (per 1M tokens)**: gpt-4o $2.50 prompt / $10.00 completion; gpt-4o-mini $0.15 / $0.60; default fallback $3.00 / $15.00.

## Slice 2 — Heuristic Router & Budget Breaker Implementation

- **Request interception & mutation**: the proxy reads the incoming JSON payload into a serde_json::Value struct and evaluates complexity heuristics: (1) does the payload contain tools or functions arrays? (2) is temperature > 0.8 (creative generation rather than strict extraction)? (3) is the calculated token length < 500? On a low complexity score, it rewrites payload["model"] = "gpt-4o-mini" (or Anthropic claude-3-haiku), updates the upstream destination URL, and forwards.
- **Budget circuit breaker**: an atomic counter in Redis via `HINCRBY tenant:{id}:spend {cost_in_cents}`; before forwarding, the proxy checks spend against the tenant's monthly limit. If exceeded: reject with HTTP 429, or forcefully override the model parameter to the cheapest available open-source model.

Code-level specifics (from the complete implementation spec):

- **ComplexityClassifier score** (0.0 dead-simple to 1.0 frontier-reasoning): tools array non-empty +0.50; response_format type json_object +0.30; total content chars > 8000 +0.40 or > 3000 +0.20; code regex (```/def /class /fn /import /SELECT /FROM ) +0.30; JSON-spec regex ("type": "object" or "properties") +0.25; capped at 1.0.
- **Downgrade rule**: complexity < 0.35 and model == gpt-4o → rewrite payload["model"] = "gpt-4o-mini" and forward modified bytes.
- **BudgetCircuitBreaker states**: `Allowed`; `EnforceCheapFallback` when current_spend + (budget_limit_usd * 0.15) >= budget_limit_usd (within 15% of depletion); `ExceededHardCap` when current_spend >= budget_limit_usd. Redis key is `tenant:{id}:monthly_spend`, incremented by estimated_cost_usd on allowed requests.

> **Status: Outdated** (2026-08-30)
> The [project direction](inferon-project-direction.md) resolves this: the spend counter is PostgreSQL (atomic UPDATE on tenant_configurations.current_spend_usd) in integer cents for the MVP; Redis fast-path + async reconciliation is the scale-out path. See the block in [Budget-Constrained Routing](budget-constrained-routing.md).

## Slice 3 — Distillation & Serving Implementation

- **Dataset generation (Python)**: a worker queries the PostgreSQL traces table for a specific system_prompt_hash where status = 200, formatting prompt/response pairs into the standard OpenAI conversational JSONL format.
- **Fine-tuning (Modal / Unsloth)**: webhook-triggered ephemeral GPU (e.g., 1x A10G on Modal); unsloth.FastLanguageModel loads meta-llama/Llama-3.2-3B-Instruct; 4-bit QLoRA with r=16, lora_alpha=16 using the SFTTrainer from Hugging Face trl. Typically under 10 minutes for 1,000 samples; pushes adapter_model.safetensors to an AWS S3 bucket.
- **Dynamic serving (vLLM)**: a persistent cluster on a base model (e.g., Llama-3.2-3B) started with --enable-lora and --max-loras 100. Hot-swap: Inferon issues a POST /v1/load_lora_adapter request pointing at the new S3 path; vLLM loads the weights into GPU memory without restarting. The Rust proxy then routes all traffic for that task to the vLLM cluster with model: "tenant_abc_lora_id".

Code-level specifics (from the complete implementation spec):

- **Dataset exporter (export_dataset.py)**: queries traces WHERE tenant_id + system_prompt_hash + status_code = 200 AND raw_completion IS NOT NULL, ORDER BY created_at DESC LIMIT 2000; writes Alpaca-style JSONL with a `conversations` field (messages + assistant content from choices[0].message.content).
- **Training (train_lora.py)**: base unsloth/Llama-3.2-3B-Instruct; 4-bit (load_in_4bit); max_seq_length 2048; r=16, lora_alpha=16, lora_dropout 0.0, bias none, use_gradient_checkpointing unsloth, random_state 3407; target modules q_proj/k_proj/v_proj/o_proj/gate_proj/up_proj/down_proj; SFTTrainer with batch size 2, gradient accumulation 4, warmup 5, max_steps 60, learning_rate 2e-4, adamw_8bit, weight_decay 0.01, linear scheduler, seed 3407; saves via save_pretrained_merged (lora).
- **vLLM client (vllm_client.rs)**: lora_name = `{tenant_id}_{task_hash[..8]}`; POST `{vllm_admin_url}/v1/load_lora_adapter` with JSON body {lora_name, lora_path}.

## Slice 4 — Self-Healing Fallback Implementation

- **Streaming output interceptor**: for requests routed to the distilled vLLM model, the proxy buffers incoming SSE chunks into a temporary memory arena instead of piping bytes straight through.
- **AST / schema validation**: once the [DONE] token arrives, the proxy parses the accumulated string against the JSON Schema from the original request's response_format parameter.
- **Transparent fallback circuit**:
  - *Success path*: validation passes → flush buffered chunks to the client (slight TTFT delay on the distilled model, but guaranteed safety).
  - *Failure path*: JSON parsing fails or validation is violated → silently discard the bad response, immediately open a new connection to the frontier fallback model (e.g., Claude 3.5 Sonnet), and stream its response. The client sees a latency spike on that request but receives a 100% valid response.
  - The failed 3B output is flagged and stored as a "negative sample" for the next DPO (Direct Preference Optimization) fine-tuning run.

Code-level specifics (from the complete implementation spec):

- **SchemaGuard (schema_guard.rs)**: uses the jsonschema crate — serde_json parse for syntax, then JSONSchema::compile + validate; the content under validation is extracted from choices[0].message.content; schema contract comes from response_format.json_schema.schema.
- **Fallback circuit (fallback_circuit.rs)**: recorded failure reasons — "Invalid JSON syntax emitted by model", "Malformed JSON schema contract", "Schema validation failed", "Corrupted non-JSON container emitted by engine", "Distilled endpoint HTTP 5xx or connection crash". On failure the payload's model is mutated to gpt-4o and re-sent to the frontier with the tenant's key.

## Slice 5 — Autonomous OmniRouting Implementation

- **Sub-millisecond prompt classification (Rust / ONNX)**: the proxy embeds the ort (ONNX Runtime) crate and loads a highly quantized embedding model (like BGE-micro). Prompts are vectorized in <3ms; Cosine Similarity against known task centroids in memory identifies the workload type and assigns it to a specific LoRA adapter.
- **Lagrangian budget solver**: predicts expected quality Q for both frontier and distilled models and solves `max_{m in M} E[Q(m, x)] subject to sum_{i=1..N} Cost(m_i, x_i) <= B_remaining`. If B_remaining is high early in the month, epsilon tightens and more traffic routes to the frontier model to maximize quality; as the budget depletes, epsilon loosens, aggressively routing acceptable traffic to the zero-cost distilled adapters to prevent a hard service cutoff.
- **Autonomous evolution loop**: a Temporal.io background workflow continuously monitors the fallback rate (from Slice 4). If a specific LoRA's fallback rate exceeds 5% (concept drift), Temporal automatically queues a new Unsloth training job on the harvested traces, tests the new adapter against a held-out gold set, and issues the vLLM load_lora_adapter command to seamlessly upgrade the endpoint.

Code-level specifics (from the complete implementation spec):

- **FastEmbedder (onnx_embed.rs)**: ort Environment + SessionBuilder with GraphOptimizationLevel::Level3 and intra_threads 4; yields a 384-d normalized vector from a (1, 128) input_ids/attention_mask pair.
- **LagrangianRouter (lagrangian_solver.rs)**: budget_remaining = max(monthly_budget_usd - current_spend_usd, 0); expected_daily_burn_allowed = budget_remaining / days_remaining_in_month; burn_ratio = expected_daily_burn_allowed / (monthly_budget_usd / 30.0). Epsilon: burn_ratio > 1.2 → 0.02 (strict, favor quality); burn_ratio < 0.5 → 0.15 (budget depleted, aggressive small-model routing); otherwise 0.05 (balanced). Routes to distilled when predicted_quality_gap <= epsilon_threshold.

## Master Architecture Matrix (latency per slice)

Hot-path ingress latency from the complete implementation spec's matrix: Slice 1 <1ms (zero-copy stream), Slice 2 <2ms (regex & payload AST), Slice 3 <2ms (routing to vLLM), Slice 4 <5ms (buffered schema check), Slice 5 <3ms (in-process ONNX embed). Serving engines progress from none (upstream proxy) through vLLM Multi-LoRA (4-bit base) and vLLM + Frontier Fallback, to a PagedLoRA GPU Pool + Dynamic Mesh; state moves from PostgreSQL traces and Redis atomic counters through S3 adapter weights and the negative trace harvester, to task embedding graphs + Temporal DAGs.

## See Also

- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [Budget-Constrained Routing](budget-constrained-routing.md)
- [Passive LoRA Distillation](passive-lora-distillation.md)
- [Multi-LoRA Serving](multi-lora-serving.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
