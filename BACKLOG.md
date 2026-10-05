# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027, 034, 035, 036, 037, 038, 039, 040 — all archived (see `.backlog/ARCHIVED/`). The full schema-guard chain is live on the durable host (default-on, per-tenant opt-out, transparent frontier fallback, guard telemetry — smoke passed live 2026-09-06). Tenants hold multiple individually revocable API keys (api_keys rows, migration 006) with an operator mint/list/revoke API; humans sign in with GitHub into signed-cookie sessions (users rows, migration 007); first-time sign-in self-serves a tenant + owner + first key (tenant_members, migration 008); platform operators (users.is_operator, migration 009, OPERATOR_GITHUB_LOGINS bootstrap) get a server-rendered operator console at /v1/dashboard/operator; owners/members manage the tenant's keys from the dashboard and invite teammates by GitHub login (claim-based member joins). ITEM-018 verified live, awaiting the PO handoff of its staged key to an external developer. **ITEM-042 (hybrid web app)** delivered: the human surfaces are a Next.js app in `web/` (landing, session-gated dashboard, operator console) served behind Caddy alongside the Rust data plane — see README → Architecture.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-018 | First real developer onboarding | improvement | **path closed (ITEM-040)** — self-serve is the only human onboarding; staged key stays valid. Remaining is a PO call: hand the key to a named developer (real-traffic data point) or archive (close-out note in item file) |
| ITEM-028 | OmniRouting: FastEmbedder | feature | **unparked for build (Option 1, 2026-10-04)** — feature-gated ORT landed (`embedder` cargo feature; macOS loop untouched); AC2 re-baselined provisionally (≤10ms on the production tier). Host measurement blocked on ITEM-041 |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-10-05 (ITEM-042 complete + archived)_ — PO chose **Option A
(hybrid)**: adopt `ixartz/SaaS-Boilerplate` for the human surfaces while the Rust
proxy/telemetry/guard/router stay the data plane; auth stays ours (A1). Slices
042a–f landed: `web/` vendored + stripped, real landing + session-gated dashboard
+ operator console, live deploy behind Caddy, CI + README. The Next.js app is the
human surface; the Rust proxy keeps `/v1/*`, `/auth/*`, `/healthz`. Platform
state: host live over HTTPS with CI/CD (deploy on every push), GitHub OAuth live,
accounts horizon (034–040), ITEM-041 and ITEM-042 archived. ITEM-028's ORT path
is feature-gated, host measurement pending. OmniRouting 029–032 remain staged.
