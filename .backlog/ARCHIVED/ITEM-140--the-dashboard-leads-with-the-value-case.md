## ITEM-140: The dashboard leads with the value case

**As** someone deciding whether to keep paying for Inferon
**I want** the first screen to say what this cost, what it saved, what it stopped
**So that** "was this worth it" is answerable in one glance

**Requested** 2026-11-09 (PO), after the competitor review: the buying trigger for a
cost product is "our AI bill jumped and nobody knows why", but the overview opened
with requests, spend, error rate, p95 TTFT and savings — five numbers, one of which
was the value case, none of them attributed.

**Delivered**

- **Three numbers lead**: **Spent**, **Saved**, **Prevented**, each with its window.
  The operational numbers (error rate, p95 TTFT, average cost) move below.
- **Both are attributed, in figures not sentences**: savings by router · fallback ·
  cache · prune; prevention by budget cap · rate limit · 85% downgrade.
- **"Prevented" is defined as all three protections** (PO, 2026-11-09): a budget
  refusal, a rate-limit refusal, and the 85% zone serving the cheap tier. Priced
  **at the decision site**, never recomputed from a later aggregate:
  `cost::projected_cost` prices the prompt (the shared chars/4 estimator) plus the
  caller's own `max_tokens`, or the documented `PROJECTED_COMPLETION_TOKENS` = 512.
  Two new columns (migration 056) carry the figure and its kind, so the dashboard
  sums what was decided rather than what someone guessed later.
  - refusals (budget, rate limit): the whole projection — the request did not run.
  - 85% downgrade: the delta between the projection at the model asked for and the
    projection at the cheap model.
- **Runtime left the customer's tabs** and moved under Operator: gateway
  microbenchmarks serve the honesty stance and the operator, not the person looking
  at their bill. The tenant keeps Overview · Spend · Cost audit · Quality ·
  Payloads · Datasets.
- `Summary.value` on the API carries `spent_usd`, `saved_usd`, `saved_by`,
  `prevented_usd`, `prevented_by` — one object, so the next surface reads the same
  numbers instead of re-deriving them.

**Tests** (5 new, 594 total): projection uses the caller's `max_tokens`; falls back
to the documented default; reads `max_completion_tokens` too; a downgrade delta is
non-negative; refused fixtures carry the new fields. Nav test asserts Runtime is no
longer among the customer tabs and is the operator's child.

**Verified live**

```
Spent $1.57 · Saved $26.87 · Prevented $0.00   (and the attribution row)
```

A disposable workspace whose key had `rpm_limit = 1` produced:

```
error_class   prevented_kind  prevented_usd  status
rate_limited  rate_limit      0.0000468600   429
```

— priced at 64 `max_tokens` on `deepseek-flash`, recorded by the decision that
refused it. The workspace was deleted afterwards.

**Honest limits**

- **Prevented is an estimate** and the product says so: the prompt is the same
  chars/4 estimate the rate limiter uses, and the completion is the caller's limit
  or 512 tokens. It is the one number on the page that is not a measurement, which
  is why it is priced where the decision happened and never back-filled.
- **A refusal by a provider price we do not know** (an unpriced model) projects at
  the default row, so it can under- or over-state. The existing unpriced-model
  warning already covers the general hazard.
- **Local savings are not in `saved_by`.** A locally served request costs zero, and
  the counterfactual ("what the cloud model would have charged") needs the model the
  client asked for at that moment; the trace does not carry it for every case. Left
  out rather than approximated — the next candidate for this row.
- No first-value moment yet ("$X avoided on your first N requests"). It wants the
  same numbers with a different frame, and the frame is a product decision.

**Source:** PO request, 2026-11-09; "prevented means all three" (2026-11-09).
