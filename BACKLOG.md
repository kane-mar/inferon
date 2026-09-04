# Product Backlog

## Product Goal

Get Inferon in front of real developers at `https://api.inferon.dev/v1` and make cost protection automatic: a durable, trustworthy deployment with **true** cost numbers, then Slice 2's automatic cost routing and budget enforcement.

Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007) is complete and archived.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-008 | DeepSeek real pricing in the cost table | bug | **done — archived 2026-09-04** |
| ITEM-009 | Trace emitted when a client aborts a stream | bug | **done — archived 2026-09-04** |
| ITEM-017 | Cache-hit-aware cost accounting | bug | **done — archived 2026-09-04** |
| ITEM-010 | Rule B repeat counts survive proxy restarts | bug | **done — archived 2026-09-04** |
| ITEM-011 | Runbook: seed tenants, dashboard URL, ops notes | improvement | **done — archived 2026-09-04** |
| ITEM-012 | smoke.sh exercises chat + dashboard endpoints | improvement | **done — archived 2026-09-04** |
| ITEM-013 | Operator view across tenants | feature | **done — archived 2026-09-04** |
| ITEM-014 | Durable deployment of api.inferon.dev | improvement | refined |
| ITEM-015 | Heuristic model router (Slice 2) | feature | **done — archived 2026-09-04** |
| ITEM-016 | Postgres budget circuit breaker (Slice 2) | feature | **done — archived 2026-09-04** |

_Last updated: 2026-09-04_ (ITEM-008..013, 015, 016, 017 archived; remaining: ITEM-014 durable deployment)
