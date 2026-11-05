## ITEM-120: From traffic to data, and the money story (payloads, evals, audit, sessions)

**As a** workspace owner
**I want** to store and export my traffic safely, test a change before shipping it,
and read the cost numbers honestly
**So that** the dashboard answers "what did that cost and did it get worse" without a
call

**Slices from ITEM-113.** Four configuration pages under `guides/config/`:

1. `09-payloads-retention-pii.md` — the capture switch, retention days, the periodic
   clean-up, PII detection and the two redaction modes (storage, wire holdout), what
   redaction does to an export.
2. `10-datasets-gold-sets-evals.md` — dataset list and JSONL export, the qualification
   rules, pinning a gold set, running an eval, and reading the regression verdict.
3. `11-cost-audit-findings.md` — the six findings one by one: what is detected, the
   line the report shows, and the setting that fixes it.
4. `12-sessions-and-spend.md` — what `X-Inferon-Session-Id` groups, why the total
   differs from the provider's own bill, and which parts are estimates.

**Acceptance Criteria**
1. Every number or finding is traced to a real field or report page, with no invented
   figures.
2. The export and gold-set pages were run on the live host, including a refusal case
   (capture off or consent missing).
3. The spend page states where a difference against a provider invoice can come from
   (rounding, cached tokens, unpriced models, failed attempts) and how to check each.
4. `python3 scripts/check-guides.py` passes; `Last verified` carries the date.
5. Live on the durable host.

**Source:** ITEM-113 breakdown, 2026-11-05.
