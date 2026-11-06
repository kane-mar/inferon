# The automatic router and its threshold

The router moves a request from the expensive model to the cheap one **before** it is
sent, when the work looks simple. It is off until you turn it on, and it only ever
applies to a pair ([models and cheaper pairs](02-models-and-tier-pairs.md)).

## Turn it on

**Routing → Controls → Automatic router**. The threshold is the only knob:

- `0` — nothing is ever simple enough; every request stays where it was asked to go.
- `0.35` — the default. Only plainly simple requests move.
- `1` — everything that has a pair moves.

## How "simple" is scored

The score is a fixed heuristic over the request, from 0 to 1. Nothing learns, nothing is
predicted, and no model is called to decide:

| The request contains | Points |
|---|---|
| `tools` or `functions` (either one) | +0.50 |
| `response_format` of any kind | +0.30 |
| code markers (```` ``` ````, `def `, `class `, `fn `, `import `, `SELECT `, `FROM `) | +0.30 |
| schema-like text (`"type": "object"`, `"properties"`) | +0.25 |
| more than 3,000 characters of message text | +0.20 |
| more than 8,000 characters of message text | +0.40 (instead of the above) |

A request scores **below** the threshold → it moves. That is why the default 0.35 keeps
tool-calling and structured-output requests where they are: either of those alone already
scores 0.30–0.50.

## What it does to a request

With the router on and a simple request for the expensive model, the trace records the
move and where the rule came from:

```json
"router_event": {"reason": "heuristic_router", "from_model": "mock-expensive",
                 "to_model": "mock-ok", "pair_source": "tenant"}
```

With the router off, the same request is served by the model it asked for and no
`router_event` appears. Both are visible in the trace, so "did the router do it?" is a
lookup, not a guess.

The budget breaker is separate and always on: past your monthly cap, a request is
refused (or moved to the cheap fallback model, if you set one) regardless of the router.

## Turning it off without losing the pairs

The pairs stay saved. Turn the router off and requests go back to the model they name;
turn it back on and the same pairs apply again. **Routing → Recent changes** records each
change with the actor and the previous value.

## How to confirm it worked

- The response's `model` is the cheap one, and the trace's `router_event` names
  `from_model` → `to_model`.
- **Spend** shows the cheap model taking the traffic that used to go to the expensive one,
  and **Savings** attributes the difference to the router.
- **Quality** answers the next question — did quality move? Compare against the same
  window before the change, or run an evaluation
  ([datasets, gold sets and evals](10-datasets-gold-sets-evals.md)).

## When to stop and ask

- A request that plainly scores under the threshold was not moved, and its trace has no
  `router_event`.
- Everything moves, including tool-calling requests, with the threshold at its default.
- The cheap model's answers are worse and you want the router to leave one prompt family
  alone — there is no per-prompt exclusion today; that is a feature request.

_Last verified 2026-11-06 against the live host (master acdaffc)._
