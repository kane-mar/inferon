# Inferon Product Roadmap (Vertical Slices)

> Sources: User-provided roadmap, 2026-08-30
> Raw: [inferon-vertical-slice-roadmap.md](../../raw/llm-inference/inferon-vertical-slice-roadmap.md); [aio-proxy-technical-specification.md](../../raw/llm-inference/aio-proxy-technical-specification.md)
> Updated: 2026-08-30

## Overview

Inferon is the productized form of the AIO-Proxy architecture ([AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)), shipped as a 30-day, five-slice vertical roadmap. Each slice delivers a user-facing capability that compounds into the fully autonomous system: a drop-in proxy at https://api.inferon.dev/v1, a heuristic router with a hard budget cap, one-click distillation, a self-healing schema guard, and finally autonomous OmniRouting.

| Slice | Delivery Window | Deliverable | Moat / Value Driver |
|-------|-----------------|-------------|---------------------|
| Slice 1 | Days 1-5 | Transparent Proxy & Cost Linter | Zero-friction ingress; immediate spend visibility |
| Slice 2 | Days 6-10 | Heuristic Router & Budget Hard-Cap | First active cost-saving mechanism |
| Slice 3 | Days 11-18 | 1-Click LoRA Distillation & Serving | End-to-end model distillation and custom deployment |
| Slice 4 | Days 19-24 | Self-Healing Schema Guard & Fallback | Eliminates production risk of small models |
| Slice 5 | Days 25-30 | Fully Autonomous Optimization (OmniRouter) | Full platform lock-in; autonomous cost/quality loop |

## Slice 1 — Transparent "Zero-Loss" Analytics & Cost Linter Proxy (Days 1-5)

A drop-in proxy URL that developers paste into their existing OpenAI or Anthropic SDK config. Responses stream with zero added latency; developers gain an instant cost-leak dashboard.

- **Ingress (Hot Path)**: Axum/Rust HTTP reverse proxy handling SSE streaming passthrough.
- **Async Telemetry**: pushes request payloads, system prompt hashes, token counts, TTFT (Time-to-First-Token), and upstream provider costs to a background queue (Tokio channel -> SQLite/PostgreSQL).
- **Linter Logic**: scans incoming payloads for cost anti-patterns — repeating identical 4k-token system prompts without provider cache headers, using GPT-4o for simple classification, sending raw unstructured JSON arrays.
- **The "Aha!" Moment**: "You spent $340 this week on 22,000 JSON extractions that could run on an 8B model for $7."

## Slice 2 — Heuristic Model Router & Budget Circuit Breaker (Days 6-10)

An active money-saving toggle plus a hard budget circuit breaker. Inferon routes simple tasks to cheaper hosted models (gpt-4o-mini, claude-3-haiku) while keeping heavy reasoning on frontier models. See [Budget-Constrained Routing](budget-constrained-routing.md).

- **Rule Engine (Hot Path)**: sub-1ms heuristic classifier checking prompt token length, tool definitions, and system prompt tags.
- **Budget State Engine**: in-memory Redis/Atomic counters tracking tenant spend in real time; approaching a monthly limit (e.g., $500) automatically enforces graceful downgrades or alerts.
- **Dynamic Upstream Client**: multiplexes requests across provider APIs based on the routing decision without breaking the client's OpenAI SDK schema.
- **The "Aha!" Moment**: a $200 budget ceiling cuts the daily bill by 40% on day 1 of activation without dropping output quality on complex prompts.

## Slice 3 — "1-Click" Assisted LoRA Distillation & Serving (Days 11-18)

A dashboard prompt cluster view with a "Distill to Custom 3B Model" button that activates once a workload accumulates 500+ successful completions. See [Passive LoRA Distillation](passive-lora-distillation.md).

- **Dataset Filter Worker**: offline job querying logged traces for a specific system prompt hash, discarding errors, and formatting prompt-completion pairs into an Alpaca/OpenAI JSONL dataset.
- **On-Demand Training Runner**: ephemeral GPU worker (Modal/RunPod or custom) running an Unsloth/PyTorch QLoRA script fine-tuning Llama-3.2-3B-Instruct or Qwen-2.5-3B-Instruct.
- **Dedicated LoRA Serving**: auto-deploys the trained adapter onto an internal vLLM cluster with dynamic adapter loading enabled.
- **Manual Cutover Toggle**: a UI switch routing 10%, 50%, or 100% of that task's traffic to the private distilled model.
- **The "Aha!" Moment**: a custom private 3B model fine-tuned on exact production data with zero ML engineering knowledge, cutting latency by 4x and per-token cost by 90%.

## Slice 4 — Self-Healing Fallback & Schema Integrity Guard (Days 19-24)

Enterprise-grade reliability for small models: guarantees 100% schema validity and zero downtime via an automated verification and fallback safety net.

- **Streaming Output Verifier**: intercepts output tokens from the distilled 3B model in real time to validate JSON syntax and field constraints against the user's expected schema.
- **Transparent Fallback Circuit**: on invalid syntax, hallucinations, or an error code, the proxy instantly reroutes the original prompt to the frontier model (GPT-4o/Claude) in the background — the end user never sees an error.
- **Negative Sample Harvester**: prompts that caused a distilled model failure are tagged as edge-case training samples for the next fine-tuning cycle.
- **The "Aha!" Moment**: developers no longer fear running smaller open-source models in production because Inferon guarantees deterministic schema fallback.

## Slice 5 — Fully Autonomous Background Optimization & OmniRouting (Days 25-30)

A fully autonomous "Inference Economics Engine": continuously clusters live traffic, automatically schedules fine-tuning jobs, hot-swaps new adapters into production without downtime, and mathematically optimizes routing using Lagrangian budget constraints.

- **Semantic Embedding Classifier**: ingress proxy embeds prompts via a quantized ONNX model (BGE-micro or ModernBERT) in <3ms to match tasks to active LoRA adapters.
- **Lagrangian Multiplier Solver**: dynamically calculates the quality-gap threshold epsilon_threshold based on remaining monthly budget, p95 latency targets, and model confidence scores.
- **Multi-Tenant PagedLoRA Serving**: shared GPU pool hosting base models in VRAM, dynamically paging tenant LoRAs in and out of host RAM on demand. See [Multi-LoRA Serving](multi-lora-serving.md).
- **Continuous Evolution Loop**: on production prompt drift detection, background pipelines automatically trigger retraining and canary deployments.
- **The "Aha!" Moment**: zero-touch optimization — models get progressively cheaper, faster, and more specialized every week without writing code.

## Relationship to the AIO-Proxy Spec

The roadmap is the phased product plan for the system the technical spec describes. Working names differ: the spec calls the product "AIO-Proxy" at https://api.aioproxy.com/v1; the roadmap calls it **Inferon** at https://api.inferon.dev/v1 — the [project direction](inferon-project-direction.md) (2026-08-30) confirms **inferon** as the working name. Two implementation details also differ between the sources (see the Status blocks in [aio-proxy-overview](aio-proxy-overview.md) and [passive-lora-distillation](passive-lora-distillation.md)): the telemetry transport (spec: Kafka/Redpanda; roadmap: Tokio channel -> SQLite/PostgreSQL) and the distillation trigger threshold (spec: N >= 1,000 samples; roadmap: 500+ completions).

## See Also

- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
- [Budget-Constrained Routing](budget-constrained-routing.md)
- [Passive LoRA Distillation](passive-lora-distillation.md)
- [Multi-LoRA Serving](multi-lora-serving.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
- [Inferon Project Direction](inferon-project-direction.md)
