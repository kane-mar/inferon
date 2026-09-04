# Product Backlog

## Product Goal (Slice 1)

Establish the zero-friction ingress pipeline and provide immediate cost visibility: a drop-in proxy at `https://api.inferon.dev/v1` that developers paste into their existing OpenAI/Anthropic SDK config, streams with zero added latency, and surfaces an instant cost-leak dashboard.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked)

| ID | Title | Type | Priority | Status |
|----|-------|------|----------|--------|
| ITEM-003 | Zero-copy SSE proxy ingress | feature | 3 | **done — archived 2026-08-31** |
| ITEM-004 | Auth & tenant context (Bearer key swap) | feature | 4 | **done — archived 2026-08-31** |
| ITEM-005 | Async telemetry pipeline | feature | 5 | **done — archived 2026-09-04** |
| ITEM-006 | Cost linter & cost calculator | feature | 6 | refined |
| ITEM-007 | Cost-leak dashboard | feature | 7 | refined |

_Last updated: 2026-09-04_ (ITEM-001–005 archived)
