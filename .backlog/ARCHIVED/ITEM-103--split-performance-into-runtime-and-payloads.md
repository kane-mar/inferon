## ITEM-103: Split Performance into Runtime and Payloads

**As a** tenant
**I want** latency and payload governance to be separate views
**So that** "is the proxy fast?" and "what happens to my prompts?" stop sharing a page

**Current state (2026-11-03).** `/dashboard/performance` renders **13 KPIs and 9
cards**: the sub-millisecond claim, proxy overhead (4), end-to-end latency (5),
transaction cost (2), a model × time heatmap, in-flight findings, the pruning
holdout, PII counts, process-wide cache counters (operator-only) and a per-model
table. Four different questions, one scroll.

**Acceptance Criteria**
1. `/dashboard/runtime` — the claim, proxy overhead, end-to-end latency, the
   heatmap and the per-model split. Nothing about payloads.
2. `/dashboard/payloads` — lint state (rules, coverage, findings, dismissals),
   the pruning policy and its holdout arms, PII counts/modes, and the capture /
   retention switches. Nothing about latency.
3. Transaction cost (per-session p50/p95 and the turns/cost table) moves to
   Spend, where "where the money goes" is the question.
4. The process-wide hot-path cache leaves the tenant surface: it is operator
   information and belongs on the operator console.
5. Each page fetches only what it renders (two read models, not one payload used
   twice), and `/dashboard/performance` redirects to `/dashboard/runtime`.
6. No number changes meaning: the same SQL that produced a metric still produces
   it, on the same window.

---

**Delivered (2026-11-03).** Live-verified on tenant kane-mar.

- `GET /api/runtime` and `GET /api/payloads` are two read models (session-scoped,
  401 without a session); `/api/performance` is gone and `/dashboard/performance`
  307s to `/dashboard/runtime`.
- Runtime renders 4 sections (claim · end to end · model × time · by model);
  Payloads renders 4 (capture & retention · lint · pruning · PII), each showing
  the switch that controls it.
- Transaction cost moved to Spend (p50/p95 cost + the turns/cost table); the
  process-wide cache no longer appears on a tenant page.
- Evals renders inside Quality; Prompts inside Datasets. `/dashboard/evals` and
  `/dashboard/prompts` 307 to their new homes. The tab bar is seven views:
  Overview · Spend · Audit · Quality · Datasets · Runtime · Payloads.
- 506 Rust tests (the runtime/payloads auth test replaced the performance one),
  `tsc`/eslint/i18n clean.
