## ITEM-140: The dashboard leads with the value case

**As** someone deciding whether to keep paying for Inferon
**I want** the first screen to say what this cost, what it saved, and what it
prevented
**So that** the product answers "was this worth it" in one glance, instead of
showing five operational metrics and burying the money case

**Why** (from `wiki/llm-inference/inferon-pmf-and-interface.md`): the buying
trigger for a cost product is "our AI bill jumped and nobody knows why". The loop a
customer runs is *see the bill → see what was avoidable → act → see it did not
come back*. The Overview page currently opens with requests, spend, error rate,
p95 TTFT and savings — five numbers of which one is the value case, and none is
attributed.

**Acceptance Criteria**

1. **Lead with three numbers, not five**: *Spent*, *Saved*, *Prevented*, each with
   its window. Spend stays; the savings KPI moves to first position beside it.
2. **Savings are attributed**: router · fallback · cache · prune · local, as one
   row of figures under the total (the data exists — `savings_kind` and the
   cache/prune events are already recorded per trace).
3. **"Prevented" is defined, not implied.** It counts what the protections stopped:
   budget refusals (requests that would have run), cheap-tier downgrades at 85%,
   and rate-limit rejections — priced at what the request would have cost. The
   definition goes in the item that implements it, because a number that estimates
   "would have" is the easiest thing in this product to get wrong, and this product
   does not ship numbers it cannot stand behind.
4. **One control per finding.** The attention strip already links errors/latency/
   waste to their control; keep that, and let the audit's six findings feed it
   directly (they already carry a dollar figure).
5. **The tenant nav loses Runtime.** Gateway microbenchmarks (and the band/drift
   analysis built for them) serve the honesty stance and the operator, not a
   customer looking at their bill. Move it under Operator; leave the tenant with
   Overview · Spend · Cost audit · Quality · Payloads · Datasets.
6. **First-value moment.** A workspace whose traffic has just started sees the value
   case ("$X avoided on your first N requests") rather than an empty grid.

**Not in scope**: a model catalogue, more charts, or any new explanatory text
(ITEM-138 settled that).

**Source:** PO request, 2026-11-09 ("review the UI to better address the compelling
advantage and to better meet users needs"), after
`wiki/llm-inference/inferon-pmf-and-interface.md`.
