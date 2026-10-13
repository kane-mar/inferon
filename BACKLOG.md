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
| ITEM-052 | Hybrid data plane (on-prem proxy + cloud control plane) | feature | **done — slice 1 (2026-10-12)** — `POST /v1/edge/traces` ingest backbone (tenant-authed batches → existing sink; cloud-authoritative cost). Live: batch persisted with recomputed cost/cache savings. Edge node mode + config sync split to ITEM-065 |
| ITEM-053 | Cache-hit telemetry & savings | improvement | **done (2026-10-12)** — persists cached prompt tokens + savings; summary/by-model expose cache hit rate + `cache_savings_usd`; dashboard KPI + Cached % column. ITEM-018 finding |
| ITEM-054 | Router axis for DeepSeek-native tiers | feature | **done (2026-10-12)** — configurable `TIER_PAIRS` (gpt-4o→mini, deepseek-v4-pro→flash); router event traced (`traces.router_event`). ITEM-018 finding |
| ITEM-055 | Quality & outcome telemetry | improvement | **done (2026-10-13)** — derived `outcome` (ok/empty/refused/error/truncated, OpenAI+Anthropic JSON/SSE) on traces; `X-Inferon-Trace-Id` + `POST /v1/quality` thumbs; `/api/quality` + `/dashboard/quality`. Live end to end |
| ITEM-056 | Latency percentiles | improvement | **done (2026-10-12)** — p50/p95/p99 of TTFT/total from Postgres `percentile_cont`; dashboard shows p95 headline + avg hint. Live: matches direct SQL (p95 4585ms vs avg 1660ms). Overhead split + p95 alerts are ITEM-066 |
| ITEM-057 | Error taxonomy & truncation visibility | improvement | **done (2026-10-13)** — `ErrorClass` (upstream_429/5xx, client_4xx, inferon_4xx/5xx, transport, timeout, guard, client_abort) on `traces.error_class` (migration 026); summary/quality expose `errors_by_class` + `truncated_rate`; by-provider shows error rate. Live: upstream 400 → `client_4xx`. Alerting split to ITEM-068 |
| ITEM-058 | Savings attribution (router/fallback/cache/frontier delta) | improvement | **done (2026-10-13)** — `traces.savings_usd`/`savings_kind` (migration 027); summary exposes router/fallback/total savings + unreduced cost (reconciles); by-model frontier cost. Live: downgrade saved $0.000082 on a $0.000041 call |
| ITEM-059 | Telemetry integrity counters | improvement | **done (2026-10-12)** — in-process counters for traces queued/persisted/dropped(by reason) + unpriced models; operator-only `GET /operator/metrics`. Live: counters move with traffic, `lost:0` |
| ITEM-060 | Throughput & capacity metrics | improvement | **new** — requests/min, in-flight gauge, telemetry queue depth, flush lag (operator endpoint) |
| ITEM-061 | Provider experience metrics | improvement | **new** — per-provider latency (p95) + error rate alongside spend/reachability; alert on degradation |
| ITEM-062 | Product activation & retention metrics | improvement | **new** — active tenants/keys, requests per developer, first-request funnel; operator-only aggregates |
| ITEM-063 | Cache-affinity routing | feature | **new** — route same-prefix requests to the provider/model that has the prefix warm; weigh cache value vs downgrade value; bounded TTL affinity |
| ITEM-064 | Adaptive policy loop (telemetry → policy → hot path) | feature | **new** — versioned, auditable, reversible policy derived from telemetry and served to the hot path; bounded, holdout-gated, kill-switchable |
| ITEM-065 | Edge node mode & config sync (hybrid data plane — slice 2) | feature | **new** — edge mode against a local provider, revocable edge token + enrollment, versioned config sync, telemetry shipping via ITEM-052, explicit offline behaviour, node health |
| ITEM-066 | Proxy overhead split & latency regression alerts | improvement | **new** — expose proxy time vs upstream time; alert on a p95 regression. Split from ITEM-056 |
| ITEM-067 | Structured-output normalization (guard on the default provider) | feature | **new** — `json_schema`→`json_object` where unsupported (DeepSeek) with the schema kept for the guard; validate the frontier replay. Finding 2026-10-13 |
| ITEM-068 | Error-rate alerting (ITEM-057 AC4) | improvement | **new** — fire an alert when the error rate (overall or per class) crosses a threshold; reuse the ITEM-020 lifecycle |
| ITEM-018 | First real developer onboarding | improvement | **done + archived (2026-10-12)** — real SDK traffic verified by dogfooding (1,226 traces, 2 tenants, streaming 200, operator isolation holds); findings captured as ITEM-053/054 |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-10-13 (ITEM-018/050/051/052/053/054/055/056/057/058/059 closed; items captured through ITEM-068)_ —
work the open set **top-down**: next the metric items **060–062**, then the
closed-loop items **063–064**, then **ITEM-065** (edge node mode), **ITEM-066**
(overhead/alerts), **ITEM-067** (structured-output normalization), **ITEM-068**
(error-rate alerting), then the staged OmniRouting cluster **029–032**.
ITEM-063 (cache affinity) and ITEM-064 (adaptive policy loop) capture the
closed-loop operation design; 064 composes 029/030/055/058/061/063 and adds
governance (versioning, bound, kill switch), it does not replace them.
