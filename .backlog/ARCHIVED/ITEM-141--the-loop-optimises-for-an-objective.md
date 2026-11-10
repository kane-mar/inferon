## ITEM-141: The loop only knew one direction, and ignored the prompt

**As** the operator of a workspace
**I want** adaptation to answer to what *this* workspace optimises for, and to act
on the prompt as well as the price
**So that** a workspace whose answers matter more than their cost gets better
answers, and a runaway bill caused by the prompt itself is addressed

**Reported** 2026-11-09 (PO): "the loop should not just drive down cost, it should
also be able to intelligently optimise prompts, and optimise for quality over cost
in the right situations."

**What was true.** `recommend_threshold` had one policy: step down when the cheap
tier is at least as good, up when it is worse, clamped to `[0, 0.9]`. Every
workspace was treated as cost-first, and the floor was zero — which is how a live
workspace reached `1.4e-17` (see ITEM-139). The loop never looked at the prompt at
all, although the pre-flight linter had been pricing exactly that waste since
ITEM-090.

**Delivered**

1. **An explicit objective** per workspace — `cost`, `balanced` (the default, and
   the behaviour that existed), `quality` — persisted on the policy (migration
   057). It decides two things:
   - **when the loop may step down**: cost accepts a small quality cost
     (`baseline − margin/2`), balanced takes parity (`baseline`), quality demands a
     measured improvement (`baseline + margin`), so in quality mode a cheap tier
     that is merely *as good* is not a reason to downgrade anything;
   - **how low it may go**: floors of 0.05 / 0.15 / 0.50, which answers the open
     question from ITEM-139 — no, the loop may not reach zero.
   Quality mode also steps **up twice as fast** on a shortfall, because that is the
   thing it exists to prevent.
2. **Prompt optimisation, as a proposal with a price.** `analyse_prompt` aggregates
   the linter's findings per workspace over the trailing week: total priced waste,
   requests affected, costliest kind. The loop publishes that as policy evidence
   with `propose_pruning` when the waste is at least $1 **and** at least 10% of
   spend. It proposes and never rewrites: pruning changes answers, so the operator
   applies it with one click and the existing holdout measures the effect.
3. **The console** shows the objective as a three-state selector and the prompt
   evidence with an *Enable pruning* action (which publishes the policy version the
   workspace's own prune endpoint expects, so freeze/rollback cover it too).

**Tests** (7 new, 599 total): each objective's floor blocks the loop; quality
refuses parity and only moves on a measured improvement; quality steps up twice as
fast; cost tolerates a margin where balanced holds still; prompt waste must clear
both an absolute and a proportional threshold before it is proposed.

**Verified live**

```
objective balanced  threshold 0.15  version 11
PUT {objective: quality} -> objective quality, version 12    (threshold kept)
PUT {objective: fastest} -> 400
console: "Optimise for [Cost | Balanced | Quality]", "Prompt waste", "Enable pruning"
```

The floor also did its job on the live workspace: the threshold that had sat at
`1.4e-17` came back up to `0.15` on the first pass under the new rule.

**Honest limits**

- **"Optimise prompts" means what the product can price**: repeated system prompts,
  oversized tool schemas, boilerplate, duplicate turns — the four the linter
  detects and prices. It does not rewrite prose, and it should not: an automatic
  rewrite of a customer's prompt is a change to their product with no evidence
  behind it.
- **The proposal is refreshed when the loop publishes.** If nothing moves, the
  last observation stands; it is not a live figure. A separate read model would fix
  that and is not worth a second table yet.
- **The objective does not yet influence the *provider* weights** (a quality-first
  workspace still demotes a provider on latency). That is the next axis, and it
  belongs with ITEM-072's per-cluster preference rather than here.
- Nothing in the UI explains the objective — three words, and the guides cover the
  rest (ITEM-138's rule).

**Source:** PO request, 2026-11-09.
