# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027 — all archived (see `.backlog/ARCHIVED/`). The full schema-guard chain is live on the durable host (default-on, per-tenant opt-out, transparent frontier fallback, guard telemetry — smoke passed live 2026-09-06). ITEM-018 verified live, awaiting the PO handoff of its staged key to an external developer.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-018 | First real developer onboarding | improvement | **verified** — AC1–AC5 system checks done live 2026-09-06; **awaiting PO handoff** of staged key to an external developer (see item file) |
| ITEM-028 | OmniRouting: FastEmbedder | feature | **staged** — 2026-09-06 engineering review: ort can't build on the dev toolchain; no real artifact meets the <3ms AC on available hardware (bge-micro-v2 measured 7.4ms). PO picks an unpark trigger (see item file) |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-033 | User accounts & logins | feature | **captured** — PO 2026-09-06; scope/ranking questions open (see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-09-06 (late)_ — guard chain ITEM-023..027 delivered and archived, smoke passed live on the durable host (transparent fallback, opt-out, guard views, AC4 hot-path measured ≈0ms with the compiled-validator cache). ITEM-028..030 staged by the 2026-09-06 engineering review (evidence in-item + REFINEMENT_LOG); pullable region is otherwise empty. ITEM-031/032 remain staged (unpark = backlog order, Slice 3 parked); ITEM-033 captured/unrefined below them.
