# Inferon Vertical Slice Roadmap

> Source: Pasted by user (Kane Mar); no public URL
> Collected: 2026-08-30
> Published: Unknown

## Slice 1: The Transparent "Zero-Loss" Analytics & Cost Linter Proxy

- Shipped Timeline: Days 1-5
- User-Facing Deliverable: A drop-in proxy URL (https://api.inferon.dev/v1) that developers paste into their existing OpenAI or Anthropic SDK config. Responses stream with zero added latency, and developers gain access to an instant cost-leak dashboard.
- Core Technical Scope:
  - Ingress (Hot Path): Axum/Rust HTTP reverse proxy handling SSE streaming passthrough.
  - Async Telemetry: Pushes request payloads, system prompt hashes, token counts, TTFT (Time-to-First-Token), and upstream provider costs to a background queue (Tokio channel -> SQLite/PostgreSQL).
  - Linter Logic: Scans incoming payloads for common cost anti-patterns (e.g., repeating identical 4k-token system prompts without provider cache headers, using GPT-4o for simple classification, sending raw unstructured JSON arrays).
- The "Aha!" Moment: The developer changes one line of config and immediately sees: "You spent $340 this week on 22,000 JSON extractions that could run on an 8B model for $7."

## Slice 2: Heuristic Model Router & Budget Circuit Breaker

- Shipped Timeline: Days 6-10
- User-Facing Deliverable: An active money-saving toggle and a hard budget circuit breaker. Inferon begins actively routing simple tasks to cheaper hosted models (e.g., gpt-4o-mini, claude-3-haiku) while keeping heavy reasoning on frontier models.
- Core Technical Scope:
  - Rule Engine (Hot Path): Sub-1ms heuristic classifier checking prompt token length, tool definitions, and system prompt tags.
  - Budget State Engine: In-memory Redis/Atomic counters tracking tenant spend in real time. If a monthly limit (e.g., $500) is approached, it automatically enforces graceful downgrades or alerts.
  - Dynamic Upstream Client: Multiplexes requests across different provider APIs based on the routing decision without breaking the client's OpenAI SDK schema.
- The "Aha!" Moment: Real-time billing drop. The developer sets a $200 budget ceiling, and Inferon cuts their daily bill by 40% on day 1 of activation without dropping output quality on complex prompts.

## Slice 3: The "1-Click" Assisted LoRA Distillation & Serving

- Shipped Timeline: Days 11-18
- User-Facing Deliverable: A dashboard prompt cluster view with an active "Distill to Custom 3B Model" button once a workload accumulates 500+ successful completions.
- Core Technical Scope:
  - Dataset Filter Worker: Offline job that queries logged traces for a specific system prompt hash, discards errors, and formats prompt-completion pairs into an Alpaca/OpenAI JSONL dataset.
  - On-Demand Training Runner: Ephemeral GPU worker (via Modal/RunPod or custom worker) running an Unsloth/PyTorch QLoRA script fine-tuning Llama-3.2-3B-Instruct or Qwen-2.5-3B-Instruct.
  - Dedicated LoRA Serving: Automatically deploys the trained adapter onto an internal vLLM cluster with dynamic adapter loading enabled.
  - Manual Cutover Toggle: A UI switch allowing the developer to route 10%, 50%, or 100% of that task's traffic to their private distilled model.
- The "Aha!" Moment: The customer launches a custom, private 3B model fine-tuned on their exact production data with zero ML engineering knowledge, cutting latency by 4x and per-token cost by 90%.

## Slice 4: Self-Healing Fallback & Schema Integrity Guard

- Shipped Timeline: Days 19-24
- User-Facing Deliverable: Enterprise-grade reliability SLA for small models. Guarantees 100% schema validity and zero downtime by introducing an automated verification and fallback safety net.
- Core Technical Scope:
  - Streaming Output Verifier: Intercepts output tokens from the distilled 3B model in real time to validate JSON syntax and field constraints against the user's expected schema.
  - Transparent Fallback Circuit: If the 3B model returns invalid syntax, hallucinations, or an error code, the proxy instantly reroutes the original prompt to the frontier model (GPT-4o/Claude) in the background. The end user never sees an error.
  - Negative Sample Harvester: Any prompt that caused a distilled model failure is tagged as an edge-case training sample for the next fine-tuning cycle.
- The "Aha!" Moment: Complete peace of mind. Developers no longer fear running smaller open-source models in production because Inferon guarantees deterministic schema fallback.

## Slice 5: Fully Autonomous Background Optimization & OmniRouting

- Shipped Timeline: Days 25-30
- User-Facing Deliverable: A completely autonomous "Inference Economics Engine." The system continuously clusters live traffic, automatically schedules fine-tuning jobs, hot-swaps new adapters into production without downtime, and mathematically optimizes routing using Lagrangian budget constraints.
- Core Technical Scope:
  - Semantic Embedding Classifier: Ingress proxy embeds incoming prompts via a quantized ONNX model (BGE-micro or ModernBERT) in <3ms to match tasks to active LoRA adapters.
  - Lagrangian Multiplier Solver: Dynamically calculates the quality-gap threshold epsilon_threshold based on remaining monthly budget, p95 latency targets, and model confidence scores.
  - Multi-Tenant PagedLoRA Serving: Shared GPU pool hosting base models in VRAM and dynamically paging tenant LoRAs in and out of host RAM on demand.
  - Continuous Evolution Loop: When production prompt drift is detected, background pipelines automatically trigger retraining and canary deployments.
- The "Aha!" Moment: Zero-touch optimization. The developer points their traffic at Inferon, and their models get progressively cheaper, faster, and more specialized every week without writing a single line of code.

## Vertical Slice Roadmap Summary

| Slice | Delivery Window | Deliverable | Moat / Value Driver |
|-------|-----------------|-------------|---------------------|
| Slice 1 | Days 1-5 | Transparent Proxy & Cost Linter | Zero-friction ingress; immediate spend visibility |
| Slice 2 | Days 6-10 | Heuristic Router & Budget Hard-Cap | First active cost-saving mechanism |
| Slice 3 | Days 11-18 | 1-Click LoRA Distillation & Serving | End-to-end model distillation and custom deployment |
| Slice 4 | Days 19-24 | Self-Healing Schema Guard & Fallback | Eliminates production risk of small models |
| Slice 5 | Days 25-30 | Fully Autonomous Optimization (OmniRouter) | Full platform lock-in; autonomous cost/quality loop |
