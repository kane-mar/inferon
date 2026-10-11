# Knowledge Base Index

## llm-inference

LLM inference optimization: proxy architecture, budget-constrained routing, distillation, and multi-LoRA serving.

| Article | Summary | Updated |
|---------|---------|---------|
| [Inferon Project Direction](llm-inference/inferon-project-direction.md) | Governing decisions (2026-08-30): Docker everywhere, Postgres, working name inferon, default AI DeepSeek, budget counter in Postgres (integer cents) | 2026-08-30 |
| [Inferon Slice Engineering Specification](llm-inference/inferon-slice-engineering-spec.md) | Implementation-level detail per slice: zero-copy SSE proxy, serde_json heuristics, Redis budget breaker, Unsloth/Modal QLoRA, vLLM load_lora_adapter, schema fallback + DPO, ONNX/Lagrangian OmniRouting; + code-level params (pricing, epsilon values, hyperparameters) | 2026-08-30 |
| [Inferon Product Roadmap](llm-inference/inferon-product-roadmap.md) | 30-day, 5-slice productization plan: cost linter proxy, heuristic router + budget breaker, 1-click distillation, self-healing schema guard, autonomous OmniRouting | 2026-08-30 |
| [AIO-Proxy: Autonomous Inference Optimizer](llm-inference/aio-proxy-overview.md) | Zero-SDK proxy combining passive LoRA distillation with budget-constrained multi-model routing; three planes, data schema, failure modes, MVP rollout | 2026-08-30 |
| [Budget-Constrained Routing](llm-inference/budget-constrained-routing.md) | OmniRouter: bi-encoder complexity classifier, quality-gap scoring, Lagrangian budget/latency solver, tenant knobs | 2026-08-30 |
| [Passive LoRA Distillation Pipeline](llm-inference/passive-lora-distillation.md) | Trace capture, curation (MinHash LSH, k-means, synthetic augmentation), QLoRA training, eval gates, canary hot-swap | 2026-08-30 |
| [Multi-LoRA Serving Cluster](llm-inference/multi-lora-serving.md) | vLLM/SGLang multi-LoRA pool, pinned base weights, paged adapters, LRU eviction, 4x L4 MVP node | 2026-08-30 |
| [Customer-Hosted LLMs](llm-inference/customer-hosted-llms.md) | Control-plane/data-plane split for reaching on-prem models: A direct endpoint, C hybrid data plane (on-prem proxy + cloud control/telemetry); B cloud edge connector deferred for latency; performance/economics | 2026-10-11 |
