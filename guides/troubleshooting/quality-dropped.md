# "Quality dropped after a downgrade"

You turned on the router or moved a prompt to a cheaper model, and the answers got worse.
The product can tell you whether they did , but only if the comparison group is there.

## Symptom

Answers are shorter, more generic, or wrong in a way they were not before, on the same
prompt. Spend went down at the same time.

## What it means

A downgrade is a hypothesis. Three things can be true at once, which is why the answer is a
measurement and not a judgement:

1. **The cheap model really is worse** for this task : the common case.
2. **The cheap model is worse only for some prompts**, and the router moved all of them.
3. **Nothing changed in quality**, and what changed is the length or format of the answer,
   which reads as worse.

## Fix

1. **Look at what moved.** A trace shows `router_event` with `from_model` and `to_model`;
   **Spend** shows the cheap model taking the traffic.
2. **Compare, do not remember.** Run an evaluation on a gold set against both models
   ([datasets, gold sets and evals](../config/10-datasets-gold-sets-evals.md)). Ten scored
   cases is the floor; forty is a verdict.
3. **Roll back the one thing you changed**, and roll back only that: raise the router
   threshold above 1 (or turn the router off), or remove the pair. **Routing → Recent
   changes** shows exactly what changed and when, so the rollback is one action.
4. **If only some prompts lost quality**, exclude that prompt family by removing its pair:
   the router has no per-prompt exclusion today, so the pair is the unit.
5. **If quality is identical and the answers are just shorter**, that is a prompt problem:
   ask for the format you want in the system prompt.

## How to confirm it worked

- The comparison's verdict is not `regression`, with the case count printed beside it.
- **Quality** returns to its previous success rate for the same window.
- The trace's `router_event` is absent again (router off) or names the model you expect.

## When to stop and ask

- Quality dropped with no downgrade in the traces : something else changed (a provider's
  model version, a prompt edit). Compare the two prompt versions.
- The evaluation says "not enough evidence": the gold set is too small to decide, and
  pinning a bigger one is the next step, not a rollback.
- You rolled back and quality did not return : the change was not the cause.

_Last verified 2026-11-06 against the live host (master acdaffc)._
