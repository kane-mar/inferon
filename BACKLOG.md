# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027, 034, 035, 036, 037, 038 — all archived (see `.backlog/ARCHIVED/`). The full schema-guard chain is live on the durable host (default-on, per-tenant opt-out, transparent frontier fallback, guard telemetry — smoke passed live 2026-09-06). Tenants hold multiple individually revocable API keys (api_keys rows, migration 006) with an operator mint/list/revoke API; humans sign in with GitHub into signed-cookie sessions (users rows, migration 007); first-time sign-in self-serves a tenant + owner + first key (tenant_members, migration 008); platform operators (users.is_operator, migration 009, OPERATOR_GITHUB_LOGINS bootstrap) get a server-rendered operator console at /v1/dashboard/operator; signed-in owners/members manage the tenant's keys from the dashboard. ITEM-018 verified live, awaiting the PO handoff of its staged key to an external developer.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-018 | First real developer onboarding | improvement | **verified** — AC1–AC5 system checks done live 2026-09-06; **awaiting PO handoff** of staged key (breakdown note in item file; ordering decision for the PO in REFINEMENT_LOG) |
| ITEM-039 | Accounts: tenant members | feature | refined (from ITEM-033) — pullable |
| ITEM-040 | Accounts: migrate out-of-band onboarding; close ITEM-018 path | chore | refined (from ITEM-033) — pullable |
| ITEM-028 | OmniRouting: FastEmbedder | feature | **staged** — 2026-09-06 engineering review: ort can't build on the dev toolchain; no real artifact meets the <3ms AC on available hardware (bge-micro-v2 measured 7.4ms). PO picks an unpark trigger (see item file) |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-09-07 (ITEM-038 delivered + archived)_ — **ITEM-038
(tenant key management) is DONE + archived**: signed-in owners/members manage
keys from the dashboard (owner-only revoke, members create; one shared mint
path; cross-tenant revoke is 404) — 176 tests green, smoke passed live (62
checks). ITEM-018 stays open at the top for its PO-gated handoff. Next in the
pullable region: ITEM-039 (tenant members), then ITEM-040 (close the
out-of-band path). PO gates unchanged: GitHub OAuth app + host env
(GITHUB_*/SESSION_SECRET/UPSTREAM_MASTER_KEY/OPERATOR_GITHUB_LOGINS) flip the
human surfaces live. OmniRouting ITEM-028..032 remain staged below.
