# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027, 034–042 — all archived (see `.backlog/ARCHIVED/`). The data plane is real and live, and the tenant-facing **control plane is delivered** (providers, per-provider models, fallback rules, routing/budget controls, spend breakdowns, SDK onboarding) — archived as ITEM-043–049. Real SDK traffic now flows (ITEM-018, archived 2026-10-12). Customer-hosted connectivity is captured as ITEM-051/052; the remaining vision work is the autonomy roadmap (OmniRouting, then distillation), which the real traffic now unblocks.

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
| ITEM-050 | Native Anthropic SDK support (provider adapter) | feature | **done (2026-10-12)** — `anthropic` kind + `POST /v1/messages`; `x-api-key`/`anthropic-version`, Anthropic JSON+SSE usage, system hashing, Claude prices; router/guard bypassed, hard cap kept. Live: forwarded to the real API (401 relayed + traced) |
| ITEM-051 | Customer-hosted endpoint (direct / BYO endpoint) | feature | **done (2026-10-12)** — bring-your-own-endpoint recipe (wiki §A), pre-add Test-connection probe, persisted per-provider reachability, live model discovery. Option A (no cloud relay) |
| ITEM-052 | Hybrid data plane (on-prem proxy + cloud control plane) | feature | **new** — run the proxy on-prem against the local model; cloud = control plane + telemetry ingest. Option C (latency-optimal; inference never leaves the network); first slice = edge telemetry ingest |
| ITEM-053 | Cache-hit telemetry & savings | improvement | **done (2026-10-12)** — persists cached prompt tokens + savings; summary/by-model expose cache hit rate + `cache_savings_usd`; dashboard KPI + Cached % column. ITEM-018 finding |
| ITEM-054 | Router axis for DeepSeek-native tiers | feature | **done (2026-10-12)** — configurable `TIER_PAIRS` (gpt-4o→mini, deepseek-v4-pro→flash); router event traced (`traces.router_event`). ITEM-018 finding |
| ITEM-055 | Quality & outcome telemetry | improvement | **new** — per-request `outcome` (ok/empty/refused/error) + optional client quality signal; quality view by model/prompt. The missing input for ITEM-029/030 |
| ITEM-056 | Latency percentiles & proxy overhead | improvement | **new** — p50/p95/p99 of TTFT/total (not just the mean) + proxy-vs-upstream overhead; p95 alertable |
| ITEM-057 | Error taxonomy & truncation visibility | improvement | **new** — classify failures (upstream_429/5xx, transport, client/inferon 4xx, guard, timeout); surface the already-persisted `truncated`/abort rate |
| ITEM-058 | Savings attribution (router/fallback/cache/frontier delta) | improvement | **new** — compute what routing/fallback/cache actually saved (counterfactual model + price delta); reconcile actual + savings = unreduced |
| ITEM-059 | Telemetry integrity counters | improvement | **new** — traces emitted vs persisted vs dropped (by reason); unpriced-model count; alert on drop rate |
| ITEM-060 | Throughput & capacity metrics | improvement | **new** — requests/min, in-flight gauge, telemetry queue depth, flush lag (operator endpoint) |
| ITEM-061 | Provider experience metrics | improvement | **new** — per-provider latency (p95) + error rate alongside spend/reachability; alert on degradation |
| ITEM-062 | Product activation & retention metrics | improvement | **new** — active tenants/keys, requests per developer, first-request funnel; operator-only aggregates |
| ITEM-063 | Cache-affinity routing | feature | **new** — route same-prefix requests to the provider/model that has the prefix warm; weigh cache value vs downgrade value; bounded TTL affinity |
| ITEM-064 | Adaptive policy loop (telemetry → policy → hot path) | feature | **new** — versioned, auditable, reversible policy derived from telemetry and served to the hot path; bounded, holdout-gated, kill-switchable |
| ITEM-018 | First real developer onboarding | improvement | **done + archived (2026-10-12)** — real SDK traffic verified by dogfooding (1,226 traces, 2 tenants, streaming 200, operator isolation holds); findings captured as ITEM-053/054 |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-10-12 (ITEM-018/050/051/053/054 closed; metrics review captured as ITEM-055..062)_ —
work the open set **top-down**: next **ITEM-052** (hybrid data plane C), then the
metric items **055 → 062**, then the closed-loop items **063–064**, then the
staged OmniRouting cluster **029–032**. ITEM-055 (quality) is the input 029/030
consume, so it lands before the OmniRouting unpark.
ITEM-063 (cache affinity) and ITEM-064 (adaptive policy loop) capture the
closed-loop operation design; 064 composes 029/030/055/058/061/063 and adds
governance (versioning, bound, kill switch), it does not replace them.
