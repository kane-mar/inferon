## ITEM-142: Restructure the interface around the customer's loop

**As** a customer with a bill that jumped
**I want** the nav to mirror what I actually do
**So that** I do not have to know the internal taxonomy to find the answer

**Requested** 2026-11-09 (PO): "use the competitive analysis to restructure the UI.
Make it so that the UI meets the needs of the user, in an intuitive manner."

**What the analysis said.** The buying trigger is a bill that jumped; the job is
*see the bill → see what was avoidable → act → confirm it did not come back*. The
interface did not follow that. It offered "Spend" and "Cost audit" as two tabs of
equal weight, which asks the customer to know that "what did we spend" and "where
did it leak" are one question. It also kept Runtime — our own microbenchmarks — in
the customer's tabs (fixed in ITEM-140).

**Delivered**

- **One Bill page.** The spend breakdown (by model, provider, key, prompt,
  transaction) and the audit's six findings now render together: the breakdown
  first, then the findings that explain it, each with its dollar figure and the
  control that changes it. One page, one window, one fetch — the two were already
  reading the same traces.
- **The nav is the loop**: Overview · Bill · Quality · Payloads · Datasets, then
  Providers · Routing · Settings as the controls, and Operator (with Runtime) for
  the platform. Five tabs instead of six, and one fewer concept to learn.
- **Old links keep working**: `/dashboard/audit` redirects to the bill carrying the
  window (`?days=`), as does the settings quick-link. Labels follow the product's
  words: "Bill", not "Spend" and "Cost audit".

**Verified live**

```
tabs: Overview | Bill | Quality | Payloads | Datasets
/dashboard/audit?days=7  ->  /dashboard/spend?days=7  (200)
bill page: "Bill … Spend by model … Cost audit … What caused the bill … Share this audit"
en 200, fr 200, landing 200, real completion 200, 0 web errors, 0 proxy errors
```

tsc, eslint, i18n-check clean (unused nav and audit strings removed).

**What this deliberately does not do**

- **No new prose.** ITEM-138's rule holds: the merge removes a concept, it does not
  add an explanation of the merged page.
- **No first-value moment yet.** A workspace with its first few requests should see
  "saved $X on your first N requests" as a moment, not a KPI. It wants the same
  numbers in a different frame, and the frame is a product decision.
- **Payloads stays a tab.** It is governance (capture, retention, PII) plus one cost
  control (pruning). Folding it into the bill would put a PII switch on a money
  page; folding it into Settings would bury the prune comparison nobody would find.
  Three of its four cards are read-only facts, so it reads as a short page.
- **Providers, Routing and Settings stay three sections.** They are three different
  jobs (connect a vendor, decide policy, manage people and keys). Collapsing them
  would trade a tab for a scroll.

**Source:** PO request, 2026-11-09.
