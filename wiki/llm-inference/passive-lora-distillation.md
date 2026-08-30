# Passive LoRA Distillation Pipeline

> Sources: User-provided technical specification, 2026-08-30
> Raw: [aio-proxy-technical-specification.md](../../raw/llm-inference/aio-proxy-technical-specification.md); [inferon-vertical-slice-roadmap.md](../../raw/llm-inference/inferon-vertical-slice-roadmap.md); [inferon-slice-engineering-spec.md](../../raw/llm-inference/inferon-slice-engineering-spec.md)
> Updated: 2026-08-30

## Overview

AIO-Proxy distills frontier behavior into small open-weight models **passively**: it learns purely from production traffic that flows through the proxy, with no extra SDK instrumentation. Traces stream off the hot path into a curation pipeline, clusters trigger autonomous QLoRA training runs, and passing adapters are hot-swapped into the serving pool behind a canary release.

## Trace Capture & Telemetry

- All requests and completions stream to a Redpanda / Kafka event log.
- The response streaming hook pushes a complete event record per request — input payload, system prompt, output tokens, time-to-first-token, completion status — with zero added latency (the client token stream is piped through via SSE in parallel).

## Autonomous Curation Pipeline

1. **Noise Filtering** — discards requests with client errors, truncated generation stops, or low downstream feedback scores.
2. **Deduplication & Clustering** — MinHash LSH de-duplication, then k-means clustering over prompt embeddings to isolate distinct workload types (e.g., JSON extraction, SQL generation, creative copywriting).
3. **Synthetic Pair Augmentation** — for thin clusters, generates edge-case variations using the frontier model to enforce schema boundaries and edge-case resilience.

## Automated Training Orchestrator (Temporal.io + Unsloth)

1. When a cluster reaches N >= 1,000 high-quality verified traces, an ephemeral cloud GPU worker is triggered (e.g., 1x L4 or 1x A10G).

> **Status: Disputed**
> The [Inferon Product Roadmap](inferon-product-roadmap.md) Slice 3 triggers distillation at **500+ successful completions** instead of the spec's N >= 1,000. Sources disagree on the trigger threshold; the roadmap also frames it as a user-initiated "Distill to Custom 3B Model" button rather than a purely automatic trigger.
2. QLoRA fine-tuning runs with Rank 16-64, Alpha 32, using cross-entropy loss against frontier model outputs. (The spec also names Axolotl as an alternative training runner alongside unsloth.)
3. An automated eval suite runs on 100 held-out golden traces:
   - **JSON Schema Validity** — must reach 100%.
   - **Semantic Cosine Similarity / Exact Match** — must reach >= 98% parity with frontier output.
4. On passing verification, the adapter artifact is pushed to S3 and the LoRA ID is registered in the proxy's active routing table.

## Adapter Registry & Hot Deployment

- Weights are uploaded to S3/MinIO; the new LoRA is registered with vLLM; the router's cost/quality graph is updated.
- `distilled_adapters` (PostgreSQL) records each artifact: `task_signature` (SHA256 of system prompt + tool schemas), `base_model` (e.g., `meta-llama/Llama-3.2-3B-Instruct`), `adapter_s3_uri`, `sample_count`, `benchmark_parity_score` (e.g., 0.985), `is_active`, `deployed_at`.
- **Canary release**: the proxy starts routing a small percentage of matching traffic to the new adapter, gradually increasing the traffic share as real-time quality verification scores remain high.

## Failure-Mode Feedback

- **Model Drift / Schema Corruption** — if JSON parsing fails on a distilled response, the request is automatically re-run against the frontier model and the LoRA's routing probability is downweighted.
- **Cold Start on New Workload Types** — new prompt templates get 100% frontier routing while traces cluster and the initial distillation job is queued.

## Roadmap Evolution (Inferon)

The [Inferon Product Roadmap](inferon-product-roadmap.md) productizes this pipeline across Slices 3-5:

- **Slice 3 (Days 11-18) — 1-Click Assisted Distillation**: a dashboard prompt cluster view shows a "Distill to Custom 3B Model" button once a workload accumulates 500+ successful completions. A dataset filter worker queries logged traces for a specific system prompt hash, discards errors, and formats prompt-completion pairs into an Alpaca/OpenAI JSONL dataset. Training runs on an ephemeral GPU worker (Modal/RunPod or custom) via an Unsloth/PyTorch QLoRA script fine-tuning Llama-3.2-3B-Instruct or Qwen-2.5-3B-Instruct. Deployment is to a dedicated vLLM cluster with dynamic adapter loading, and a **manual cutover toggle** lets the developer route 10%, 50%, or 100% of that task's traffic to the private model (complementing the spec's automatic canary).
- **Slice 4 (Days 19-24) — Self-Healing Fallback & Schema Integrity Guard**: a streaming output verifier intercepts distilled-model output tokens in real time to validate JSON syntax and field constraints; on invalid syntax, hallucinations, or an error code, a transparent fallback circuit instantly reroutes the original prompt to the frontier model (GPT-4o/Claude) in the background — the end user never sees an error. A **negative sample harvester** tags every prompt that caused a distilled-model failure as an edge-case training sample for the next fine-tuning cycle (used for DPO in the detailed spec).
- **Slice 5 (Days 25-30) — Continuous Evolution Loop**: when production prompt drift is detected, background pipelines automatically trigger retraining and canary deployments.

## Engineering Implementation (Detailed Spec)

Concrete mechanics from the [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md):

- **Dataset worker**: a Python worker queries the PostgreSQL traces table for a specific system_prompt_hash where status = 200 and formats prompt/response pairs into the standard OpenAI conversational JSONL format.
- **Training run**: webhook-triggered ephemeral GPU (e.g., 1x A10G on Modal); unsloth.FastLanguageModel loads meta-llama/Llama-3.2-3B-Instruct; 4-bit QLoRA with r=16, lora_alpha=16 via the SFTTrainer from Hugging Face trl. Typically under 10 minutes for 1,000 samples; the adapter_model.safetensors artifact is pushed to AWS S3.
- **Schema guard (Slice 4)**: SSE chunks from the distilled model are buffered into a temporary memory arena; after the [DONE] token, the accumulated string is parsed against the JSON Schema in the original request's response_format. On validation failure the bad response is silently discarded and a new connection opens to the frontier fallback (e.g., Claude 3.5 Sonnet) — the client sees a latency spike but a 100% valid response. Failed outputs are stored as a "negative sample" for the next DPO (Direct Preference Optimization) run.
- **Evolution loop (Slice 5)**: a Temporal.io workflow monitors the fallback rate; when a LoRA's fallback rate exceeds 5% (concept drift), it queues a new Unsloth training job on the harvested traces, tests against a held-out gold set, and issues the vLLM load_lora_adapter command to seamlessly upgrade the endpoint.

Code-level specifics (from the [complete implementation spec](inferon-slice-engineering-spec.md)):

- **Dataset exporter (export_dataset.py)**: queries traces WHERE tenant_id + system_prompt_hash + status_code = 200 AND raw_completion IS NOT NULL, ORDER BY created_at DESC LIMIT 2000; writes Alpaca-style JSONL with a `conversations` field (messages + assistant content from choices[0].message.content).
- **Training (train_lora.py)**: base unsloth/Llama-3.2-3B-Instruct; 4-bit; max_seq_length 2048; r=16, lora_alpha=16, lora_dropout 0.0, bias none, gradient checkpointing unsloth, random_state 3407; target modules q_proj/k_proj/v_proj/o_proj/gate_proj/up_proj/down_proj; SFTTrainer: batch size 2, gradient accumulation 4, warmup 5, max_steps 60, learning_rate 2e-4, adamw_8bit, weight_decay 0.01, linear scheduler, seed 3407; save_pretrained_merged (lora).
- **Schema guard (schema_guard.rs)**: jsonschema crate — serde_json parse then JSONSchema::compile + validate on choices[0].message.content; contract from response_format.json_schema.schema. Recorded failure reasons: "Invalid JSON syntax emitted by model", "Malformed JSON schema contract", "Schema validation failed", "Corrupted non-JSON container emitted by engine", "Distilled endpoint HTTP 5xx or connection crash".

## See Also

- [Budget-Constrained Routing](budget-constrained-routing.md)
- [Multi-LoRA Serving](multi-lora-serving.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
