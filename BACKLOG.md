# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027, 034–042 — all archived (see `.backlog/ARCHIVED/`). The data plane is real and live; the **tenant-facing control plane is not** — a tenant cannot bring their own provider, choose models, set fallback rules, or configure routing/budget from the UI, and the onboarding flow does not yet back the landing promise. Those gaps are captured as ITEM-043..048 and rank **above** putting the service in front of real external developers.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-043 | Provider provisioning (bring your own provider) | feature | **done (2026-10-06)** — encrypted provider creds (`deepseek`/`openai`/`openai-compatible`), proxy resolution, `/dashboard/providers` add/list/test/**rotate**, optional org/headers, last-updated; native Anthropic re-scoped to ITEM-050 |
| ITEM-044 | Model catalog & per-provider selection | feature | **done + reworked (2026-10-07)** — catalog + `tenant_models` selection **per provider**; models live under `/dashboard/providers` (the standalone Models tab is gone); provider resolution is model-aware; disabled models refused (typed 400) |
| ITEM-045 | Fallback & routing rules | feature | **done (2026-10-06)** — ordered per-tenant fallback chain on retryable failures (429/5xx/transport), traced + configured at `/dashboard/routing`; per-step triggers + guard trigger deferred |
| ITEM-047 | Dashboard — provider/model/key spend & limits | improvement | **done (2026-10-06)** — spend by provider/model/key/prompt with period deltas + inline spike badge; per-key/per-provider monthly limits (owner UI) with ITEM-020 alerts; onboarding empty state |
| ITEM-048 | SDK onboarding flow ("connect your SDK") | feature | **done (2026-10-06)** — `/dashboard/onboarding`: mint key → base URL → snippets (tenant model) → first-request detection; links to providers (models live there now) |
| ITEM-046 | Routing policy & budget controls (UI) | feature | **done (2026-10-06)** — `/dashboard/routing` controls: budget cap + cheap fallback, guard toggle, router toggle + threshold, live spend-vs-cap at a glance; owner-only writes, applied on the next request (no redeploy). Audit trail deferred |
| ITEM-049 | Own the app shell — retire the template, one operator console | tech-debt | **done (2026-10-06)** — Inferon-owned `features/shell/` (header/nav/footer/page primitives); `templates/`, `Centered*`, `(center)` group and the `/` flourish gone; one operator console (Next), backend JSON-only |
| ITEM-050 | Native Anthropic SDK support (provider adapter) | feature | **new** — `/v1/messages` ingress + Anthropic adapter (`x-api-key`, different envelope/usage); the proxy/router/guard/cost are OpenAI-shaped (re-scoped out of ITEM-043) |
| ITEM-051 | Edge connector — reach customer-hosted (on-prem) LLMs | feature | **new** — outbound WSS connector so the hosted proxy can serve a model on a customer LAN; first slice = "bring your own endpoint" guide + connectivity check. See `wiki/llm-inference/local-llm-hosting-and-edge-connector.md` |
| ITEM-018 | First real developer onboarding | improvement | **PO call** — hand the staged key to a real developer; now sits *after* the control plane (043–048) |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-10-11 (local-LLM connectivity captured as ITEM-051)_ —
remaining: ITEM-050 (native Anthropic adapter), ITEM-051 (edge connector),
ITEM-018 (PO handoff), and the staged OmniRouting cluster (029–032).
