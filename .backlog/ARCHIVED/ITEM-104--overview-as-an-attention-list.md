## ITEM-104: Overview as an attention list, not a report index

**As a** tenant
**I want** the landing page to tell me what is wrong and what to do
**So that** I do not have to open five tabs to find out

**Current state (2026-11-03).** The overview is 7 KPIs + 6 cards, five of which
restate another tab (top spend → Spend, waste → Audit, quality → Quality,
performance → Runtime, dataset → Datasets). Nothing ranks what to look at first.

**Acceptance Criteria**
1. At most five KPIs: requests, spend, error rate, p95 TTFT, savings — each with
   its window and a definition on hover.
2. One trend chart, unchanged in behaviour.
3. An **Attention** strip above the chart: the top findings across waste, lint,
   quality and latency, ordered by dollar impact, each naming the control that
   fixes it and linking there.
4. The duplicated mini-cards are gone; the tabs are the detail.
5. Empty tenant: the attention strip is absent and the existing onboarding
   empty state stands.
