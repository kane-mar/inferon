## ITEM-019: Dashboard Polish for External Developers

**As a** developer evaluating Inferon
**I want** a dashboard that looks trustworthy and reads clearly at a glance
**So that** the cost story (the "aha!" moment) lands instead of being buried in a raw table

**Acceptance Criteria:**
1. All dashboard views render correctly over the public HTTPS URL — no mixed content, no localhost/internal references anywhere in the HTML or fetched data
2. Empty state is meaningful ("no requests yet — point your baseURL at us and make one") instead of bare zeros
3. Currency renders readably across magnitudes (e.g. $0.000016, not 1.6e-5) and cost totals are consistent with the trace-level math (ITEM-008)
4. The window selector (days) works for 7/30/90 on every view; each view's default is sensible
5. Layout is clean on a laptop screen and the waste view explains *why* something is a leak (one-line plain-English per rule, linking back to the request)

**Technical Notes:**
- No new backend surface intended; scope is presentation + any missing window plumbing on the HTML shell
- Extend `scripts/smoke.sh` where the checks are mechanical (window params, empty state, currency format); visual judgement stays a manual pass
- If real onboarding (ITEM-018) surfaces a specific dashboard complaint, that outranks this item's invented gloss — fix the real one first
