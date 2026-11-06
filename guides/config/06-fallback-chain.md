# A fallback chain that actually fires

A fallback chain says: if this model fails in a way worth retrying, try this one next.
It is off by default — a failing request returns its error.

**Routing → Fallback chain**. The first step is always the model your request asked for;
you add the ones after it.

## What each step handles

Every step declares which failures it takes, and a failure moves to the **next step that
handles that class** — not simply the next step:

| Handles | Means |
|---|---|
| `connection` | the provider could not be reached (refused, DNS, TLS, timeout) |
| `429` | the provider rate-limited us |
| `5xx` | the provider returned a server error |
| `checker` | the answer broke a JSON-schema contract, and this step should be tried for the replay |

So a chain of `[A (429), B (5xx)]` sends a `429` to B, and a `5xx` to B as well; a step
that handles only `5xx` never receives a `429`.

A **4xx from the provider is never retried**: the request itself is wrong (a bad model
name, a field the provider refuses), so sending it somewhere else just moves the
problem.

## What the client gets

A successful fallback is invisible: `200`, one answer, no indication that a second call
happened. The trace records the move:

```json
"routing_event": {"step": 1, "reason": "upstream_429",
                  "from_model": "mock-429", "to_model": "mock-ok"}
```

If every step fails, the client receives the **last** attempt's error — the real one, not
an invented summary. With no `checker` step in the chain, a failed schema check is
replayed on the deployment's frontier model instead.

## Ordering that works

1. **Same provider, different model** first: a rate limit on one model rarely applies to
   the other, and the answer quality is known.
2. **A different provider** next: this is the step that survives a provider outage.
3. **A model on your own hardware** last if you have one: slower, but it cannot rate-limit
   you and it never goes down for maintenance.

Keep the chain short. Each step is another chance to serve a *worse* answer, and a chain
that always fires means the first step is the wrong model.

## How to confirm it worked

- The response is `200` and its `model` is the step that served it, not the one you asked
  for.
- The trace carries `routing_event` with `step`, `reason` and both model names.
- **Quality → Errors by cause** still counts the original failure (`upstream_429`), so a
  chain that fires often is visible as a rising count even though clients see `200`.

## When to stop and ask

- A `429` or `5xx` reaches the client although a later step handles that class — check the
  chain was saved under **Recent changes**.
- Every step fires on every request: the first model is failing systematically, and the
  chain is hiding it. Fix the first step; do not lengthen the chain.
- The chain is exhausted and the client gets an error naming a model it never asked
  for — that is the last step's error, [by design](#what-the-client-gets).

_Last verified 2026-11-06 against the live host (master acdaffc)._
