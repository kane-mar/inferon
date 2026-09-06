# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023, 024 — all archived (see `.backlog/ARCHIVED/`). ITEM-018 verified live, awaiting the PO handoff of its staged key to an external developer.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-018 | First real developer onboarding | improvement | **verified** — AC1–AC5 system checks done live 2026-09-06; **awaiting PO handoff** of staged key to an external developer (see item file) |
| ITEM-025 | Schema guard: transparent fallback circuit | feature | refined |
| ITEM-026 | Schema guard: guard telemetry | feature | refined |
| ITEM-027 | Schema guard: activation + regression safety | feature | refined |
| ITEM-028 | OmniRouting: FastEmbedder | feature | refined |
| ITEM-029 | OmniRouting: task-centroid matching | feature | refined |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | refined |
| ITEM-033 | User accounts & logins | feature | **captured** — PO 2026-09-06; scope/ranking questions open (see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-09-06_ — guard chain progressed: ITEM-023 (buffered validation seam) + ITEM-024 (schema compile + validate) delivered and archived; ITEM-025..027 remain, then OmniRouting ITEM-028..030. ITEM-031/032 are staged and not pullable today (unpark = backlog order). ITEM-021/022 parents archived as superseded; their slices (ITEM-023..030) refined from the engineering spec — see `REFINEMENT_LOG.md`.
