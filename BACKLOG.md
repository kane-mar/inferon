# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017 — all archived (see `.backlog/ARCHIVED/`).

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-018 | First real developer onboarding | improvement | refined |
| ITEM-019 | Dashboard polish for external developers | improvement | refined |
| ITEM-020 | Cost anomaly alerts | feature | refined |
| ITEM-023 | Schema guard: buffered validation seam | feature | refined |
| ITEM-024 | Schema guard: compile + validate | feature | refined |
| ITEM-025 | Schema guard: transparent fallback circuit | feature | refined |
| ITEM-026 | Schema guard: guard telemetry | feature | refined |
| ITEM-027 | Schema guard: activation + regression safety | feature | refined |
| ITEM-028 | OmniRouting: FastEmbedder | feature | refined |
| ITEM-029 | OmniRouting: task-centroid matching | feature | refined |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | refined |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-09-05_ — ITEM-021/022 split into pullable slices (ITEM-023..032; parents superseded in `.backlog/ARCHIVED/`). ITEM-031/032 are staged and not pullable today. All slices were refined from the engineering spec — see `REFINEMENT_LOG.md` 2026-09-05.
