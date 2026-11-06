# Context pruning and the comparison group

Pruning removes parts of a conversation before it is sent, to spend fewer prompt tokens.
It changes the answer, so it is **off by default**, measured against a control group, and
turned off automatically when it makes answers worse.

## What it removes

Only two things, and never the last turn or your tool definitions:

- **Exact duplicate turns** — a message whose role and content repeat an earlier one.
- **The oldest stale turns** — only when you set a prompt-token ceiling; the oldest go
  first until the payload fits.

**Routing → Context pruning**. Enable it, choose which rules apply, and set the ceiling if
you want one. The default holdout is 10%.

## The comparison group

A share of the traffic is sent **unpruned** on purpose. That is how the product knows
whether pruning helped: two arms, same requests, one measurement.

- The arm is chosen by a deterministic hash of the workspace and prompt, so the same
  prompt stays in the same arm — it does not flip between requests.
- The trace records which arm a request was in (`prune_event.held_out`) and what was
  removed (`kinds`, `turns`, `bytes`, `tokens`, `usd`).
- **Payloads → Pruning** shows both arms and the difference in success rate.

Without a holdout there is no comparison, and a "saving" with no quality check is a guess.

## The automatic rollback

Every 24 hours, for any workspace with at least 20 requests in each arm: if the pruned arm
is **5 percentage points or more worse** in success rate, the policy is disabled
automatically and the reason is recorded:

```json
{"why":"pruning_regressed","pruned_ok":14,"pruned_requests":30,
 "holdout_ok":27,"holdout_requests":30,"disabled_version":3}
```

That is the kill switch doing its job. It is not an error: it means pruning did not pay for
itself on this traffic, with this evidence.

## Reading the result honestly

1. **Success rate is not quality.** A pruned answer can succeed and be worse. Before
   trusting a saving, run an evaluation on a gold set
   ([datasets, gold sets and evals](10-datasets-gold-sets-evals.md)).
2. **Look at the token saving per request**, not the total. A 30% saving on a prompt that
   was already cheap is a rounding error.
3. **Long conversations benefit most**; one-shot prompts have nothing to prune.

## How to confirm it worked

- **Payloads → Pruning** shows both arms with their request counts and success rates.
- Traces carry `prune_event` with `held_out` and the removed amounts.
- **Routing → Recent changes** records the policy change; a rollback appears as the policy
  version changing with the regression evidence attached.

## When to stop and ask

- Pruning is enabled and no trace carries `prune_event` — the policy may not be on the
  active version yet.
- The two arms have very different request counts with a large holdout configured.
- The rollback fired and you disagree with it: the evidence is in the policy, and the
  numbers it used are the counts above.

_Last verified 2026-11-06 against the live host (master acdaffc)._
