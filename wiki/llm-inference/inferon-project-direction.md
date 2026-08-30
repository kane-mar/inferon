# Inferon Project Direction

> Sources: Kane Mar (project decisions), 2026-08-30
> Raw: [2026-08-30-inferon-project-direction.md](../../raw/llm-inference/2026-08-30-inferon-project-direction.md); [2026-08-30-inferon-budget-counter-decision.md](../../raw/llm-inference/2026-08-30-inferon-budget-counter-decision.md); [2026-08-30-inferon-toolchain-decision.md](../../raw/llm-inference/2026-08-30-inferon-toolchain-decision.md); [2026-08-30-inferon-toolchain-scope-clarification.md](../../raw/llm-inference/2026-08-30-inferon-toolchain-scope-clarification.md)
> Updated: 2026-08-30

## Overview

The governing project decisions for the Inferon platform, recorded 2026-08-30. These override spec-level working assumptions where they conflict; the source documents in raw/ remain the historical record.

## Decisions

1. **Docker everywhere** — every component (Rust/Axum proxy, background workers, Postgres, Redis, training/serving) ships as a container. The deployment unit is an image.
2. **Postgres** — PostgreSQL is the telemetry/control-plane database, resolving the Disputed telemetry transport toward PostgreSQL (2-of-3 sources already used it; the roadmap's "SQLite/PostgreSQL" is now just PostgreSQL).
3. **Working name: inferon** — matches the roadmap; the AIO-Proxy name (and api.aioproxy.com) from the original spec is retired.
4. **Default AI: DeepSeek** — the live frontier/default model is DeepSeek. The spec's cost table (gpt-4o $2.50/$10.00, gpt-4o-mini $0.15/$0.60, default $3.00/$15.00 per 1M tokens) and routing defaults (claude-3-5-sonnet fallback) remain source-documented, but the operational default is DeepSeek — the cost table and `frontier_fallback_model` need DeepSeek rows when building (DeepSeek pricing is not yet specified in any source).
5. **Budget spend counter: PostgreSQL (MVP)** — the circuit-breaker spend counter is an atomic UPDATE on `tenant_configurations.current_spend_usd`, not Redis; Redis fast-path + async reconciliation is the documented scale-out path (typically 10k+ req/s). Values are integer cents, never float USD. See [2026-08-30-inferon-budget-counter-decision.md](../../raw/llm-inference/2026-08-30-inferon-budget-counter-decision.md).
6. **Toolchain: Rust + bash only** — no Python, no Ruby in the Inferon codebase (proxy, workers, tooling, scripts); SQL for Postgres migrations is unaffected. Scope clarification (same day): the constraint applies **strictly to the Inferon codebase** — external tools that may use Python are fine. So the implementation spec's Slice 3 Unsloth/PyTorch training workers (Modal/RunPod) stay Python as external tooling; Inferon-owned Slice 3 code (dataset export, orchestration) is reimplemented in Rust/bash. See [2026-08-30-inferon-toolchain-decision.md](../../raw/llm-inference/2026-08-30-inferon-toolchain-decision.md) and [2026-08-30-inferon-toolchain-scope-clarification.md](../../raw/llm-inference/2026-08-30-inferon-toolchain-scope-clarification.md).

## Effect on the Wiki

- [aio-proxy-overview](aio-proxy-overview.md): the telemetry-transport dispute is superseded — PostgreSQL wins; the Kafka/Redpanda design is marked Outdated.
- [budget-constrained-routing](budget-constrained-routing.md): the operational frontier default is DeepSeek; claude-3-5-sonnet remains the spec's documented fallback.
- The Redis spend-counter format dispute (HINCRBY vs monthly_spend) is **resolved** by Decision 5 — PostgreSQL atomic counter in integer cents for the MVP; both Redis variants are superseded.
- The toolchain constraint (Decision 6) supersedes the Python-based Slice 3 implementation details in the specs ([passive-lora-distillation](passive-lora-distillation.md)): Inferon-owned code (dataset export, orchestration) is reimplemented in Rust/bash; training workers may use Python as external tooling (scope clarification).

## See Also

- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
- [Budget-Constrained Routing](budget-constrained-routing.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
