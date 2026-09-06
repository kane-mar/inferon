## ITEM-030: OmniRouting — Lagrangian Budget Solver (Slice 5 · was 022c)

**As a** product owner
**I want** the quality-gap threshold to adapt to remaining budget automatically
**So that** the router spends quality early in the month and protects the budget cap late in the month — with no human tuning

**Acceptance Criteria:**
1. Epsilon is computed from budget burn per the spec formulas (`lagrangian_solver.rs`): `burn_ratio = budget_remaining/days_left_in_month ÷ (monthly_budget/30)`; >1.2 → eps 0.02 (favor quality), <0.5 → eps 0.15 (aggressive cheap routing), else 0.05 (balanced)
2. It replaces the static ITEM-015 downgrade threshold as the routing decision input; the spec's ITEM-016 integer-cents counters (`current_spend_cents`/`budget_limit_cents`) are the only spend inputs — no new counter
3. Route to the cheap tier when predicted quality gap ≤ epsilon; when budget is healthy the frontier tier keeps traffic (epsilon tight), when depleted the cheap tier absorbs traffic before the ITEM-016 hard 429 cutoff
4. Every routing decision records the epsilon + rationale on the trace; an operator view shows epsilon over time per tenant
5. Unit tests cover the schedule boundaries (TDD — healthy/balanced/depleted budget) and an end-to-end scenario proves month-end loosening actually precedes the 429 cap

**Technical Notes:**
- Depends on ITEM-028/029 (task signal). The epsilon schedule formulas (AC1/AC5) are pure and testable, but as a *routing* input (AC3: route when predicted quality gap <= epsilon) it consumes 029's quality-gap prediction, which needs real centroids; without it, replacing the working ITEM-015/016 cost protection would be unvalidated. Staged with 028/029 by the 2026-09-06 engineering review. The epsilon-schedule unit tests (AC5 TDD) are ready to write the moment the chain is pulled.
- Parent: ITEM-022 (archived), slice 022c — the OmniRouting core's completion slice
- Depends on ITEM-028/029 (task signal), the ITEM-015 router and ITEM-016 budget breaker
- Spec assumes gpt-4o/claude frontier; in the DeepSeek-only world frontier vs cheap = original requested model vs downgrade target (same convention as ITEM-025)
