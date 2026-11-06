# Datasets, gold sets and evals: from real traffic to a tested change

The loop this page describes is the reason to capture payloads at all: your own traffic
becomes a fixed exam, and every change — a cheaper model, a router threshold, a new prompt
— can be put to that exam before you trust it.

Capture must be on (**Routing → Controls → Store prompts and answers**); without it
nothing here has material.

## 1. What counts as an example

**Datasets** lists your traffic grouped by system prompt (or by task cluster when the
gateway has assigned one). Not every request is an example — a pair is **qualified**
only when:

- the outcome was `ok`,
- the answer was complete (not cut short),
- the answer was not marked bad with a thumbs-down,
- and, when the request carried a JSON-schema contract, the answer satisfied it.

Everything else is counted as **excluded**, with the reason stored on the trace — `capped`
(the captured answer hit the size cap), `truncated`, the outcome itself (`empty`,
`refused`, `error`), `guard_failed` (the checker's replay failed too) or `schema_invalid`
(the answer did not satisfy the request's contract). A small dataset is explained rather
than mysterious. A thumbs-down is a separate signal: it excludes the pair from the
audit's quality counts, and the trace keeps the thumb on `trace_quality`.

## 2. Export it

**Datasets → Export** streams a JSONL file, one example per line, with provenance (model,
prompt hash, cost, timestamp). Two things to know:

- The export honours the capture switch and the PII storage mode: redacted payloads stay
  redacted, and with redaction off they are not redacted for you.
- `prompt=<name>` scopes the export to one labelled prompt's versions.

## 3. Pin a gold set

**Quality → Evals → Pin gold set**. A gold set is **immutable and versioned**: the inputs
and their references are copied out of the traffic at that moment, so retention deleting
the originals does not change what a later run measured. One case per prompt hash keeps
the set from being dominated by a single repeated prompt.

Pin it when the traffic looks good, not when it looks plentiful.

## 4. Run an evaluation

**Quality → Evals → Run**. The gateway sends the gold set's exact inputs to the model you
choose, bounded by cases and by spend, and scores each answer deterministically:

| Scored by | When |
|---|---|
| `exact` | the case has a pinned reference |
| `schema` | the request carried a contract and the answer satisfies it |
| `outcome` | neither; the outcome signal decides |

There is **no judge model** — a second model's opinion is another opinion, not evidence.
Each case's latency and cost are recorded with the run.

A run that loses cases (provider error, budget stop) is reported as `partial`, never scored
as zero: a partial run is a smaller exam, not a worse model.

## 5. Compare two runs

**Quality → Evals → Compare** returns a verdict between a baseline and a challenger:

```json
{"verdict":"regression","baseline":0.87,"challenger":0.79,
 "scored":{"baseline":30,"challenger":30}}
```

The rule: at least **10 scored cases per run**, and a challenger **5 percentage points or
more below** the baseline is a regression. Fewer cases than that and the answer is
"not enough evidence", which is an honest verdict rather than a pass.

Eval calls are written to the eval tables only, never to `traces`, so an evaluation cannot
move your spend, routing or policy.

## How to confirm it worked

- The gold set has a version and a case count, and the run reports the same count as
  `scored` (or explains the shortfall).
- Each case names its scoring method, so you know whether `exact` or `outcome` decided.
- The comparison's verdict matches the numbers beside it.

## When to stop and ask

- **Datasets** is empty with capture on: check that the window is long enough and the
  traffic qualified (every excluded reason is shown).
- A run returns `partial` every time: the spend or case bound is too tight.
- The comparison says "not enough evidence" — pin a bigger gold set; ten cases is the
  floor, not a target.

_Last verified 2026-11-06 against the live host (master acdaffc)._
