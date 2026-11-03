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
