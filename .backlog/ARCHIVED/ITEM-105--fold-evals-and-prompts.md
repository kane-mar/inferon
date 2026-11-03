## ITEM-105: Fold Evals into Quality and Prompts into Datasets

**As a** tenant
**I want** each surface where its question is asked
**So that** the tab bar says what the product is for

**Current state (2026-11-03).** Eight equal-weight tabs mix views (Spend, Audit,
Quality, Performance, Datasets) with workflows (Evals, Prompts), and Overview is
just another tab.

**Acceptance Criteria**
1. Tabs: Overview · Spend · Audit · Quality · Datasets · Runtime · Payloads (7).
2. Evals renders inside Quality (a gate is a quality question) and Prompts inside
   Datasets (a registry is a view over the same hashes) — as sections, not nested
   routes, so a deep link still lands on the right page.
3. `/dashboard/evals` and `/dashboard/prompts` redirect to their new homes.
4. The tab bar highlights the parent tab on those pages.

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
