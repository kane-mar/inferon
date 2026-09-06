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
| ITEM-021 | Self-healing schema guard (Slice 4) | feature | captured |
| ITEM-022 | Autonomous OmniRouting (Slice 5) | feature | captured |

_Last updated: 2026-09-05_ — replenished (Rule 5): ITEM-018..020 refined (live-product, small at top), ITEM-021/022 captured (Slice 4/5, chunky at bottom). Slice 3 (LoRA distillation) deliberately **not** backlogged — parked last by PO (see REFINEMENT_LOG 2026-09-05). Archived ITEM-008..017 remain in `.backlog/ARCHIVED/`.
