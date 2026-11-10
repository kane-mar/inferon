# Budgets and rate limits that protect you without blocking real traffic

Two different protections, often confused:

- A **budget** is about money. It stops spending past a cap.
- A **rate limit** is about speed. It stops one key from sending more than a set number
  of requests or tokens per minute.

You can have either, both, or neither. Neither is on by default beyond the hard cap you
set yourself.

## The monthly budget

**Routing → Controls → Monthly budget (USD)**. At a glance the card shows
`spent of limit`.

What happens as you approach it:

| Spend | What the gateway does |
|---|---|
| below 85% | nothing; requests are served normally |
| 85% and above | if **Cheap fallback model** is set, requests move to it |
| at 100% | requests are refused (`429`) until the month rolls over or you raise the cap |

The 85% step is the useful one: it degrades to a cheaper model instead of stopping, which
is usually what an application wants. Without a cheap fallback model, 85% changes
nothing and 100% refuses.

**0 means no cap.** That is the default for a new workspace.

## Per-key rate limits

**Settings → API keys → Requests per minute / Tokens per minute**, per key. `0` means no
limit, and a key with both at `0` never touches the limiter at all.

A refusal is typed, counted and carries its own window:

```text
HTTP/2 429
retry-after: 53
x-ratelimit-limit: 1
x-ratelimit-remaining: 0
x-ratelimit-reset: 53
```

```json
{"error":{"message":"requests per minute exceeded for this key","type":"rate_limited"}}
```

The attempt is recorded on the trace as `rate_limited`, so **Quality → Errors by cause**
counts it, and this one carries `x-inferon-trace-id` like every other answer the gateway
generates. Nothing was sent upstream, so nothing is billed.

## Picking the numbers

Guess nothing; measure for a day and read it back:

1. **Requests per minute per key**: look at **Runtime → Requests** over 24h. A cap at
   roughly 2× your busiest minute protects against a runaway loop and never fires in
   normal traffic.
2. **Tokens per minute**: the same idea, from **Spend → Tokens**. One retry storm is
   usually 10× a normal minute, so 3–4× the peak is a good first cap.
3. **Monthly budget**: your planned spend, not your worst month. The 85% fallback exists
   so hitting the cap is a downgrade rather than an outage.
4. **One key per service.** A shared key means one service's burst starves the others,
   and a revocation takes everything down at once.

## What the client sees at the cap

- **Rate limited**: `429` with `retry-after`; retrying earlier just extends the wait.
- **Budget exhausted**: `429` with type `rate_limited` as well , the same retry
  behaviour, but raising the per-minute limit will not help; the monthly cap will.

Both are visible in **Quality → Errors by cause** as `rate_limited`.

## How to confirm it worked

- The key's row shows the limit you set, and **Last used** keeps moving under normal
  traffic (so the cap is not firing).
- A deliberate burst gets `429` with `retry-after` and the request appears in the report.
- **Routing → Recent changes** shows who changed a budget or limit, and what it was.

## When to stop and ask

- A `429` arrives while both the key's limits and the workspace default are `0`, and the
  body is the provider's own message , that is the provider rate-limiting you, not us
  ([429: our limit, or the provider's](../troubleshooting/too-many-requests.md)).
- Requests are refused at 100% of a budget you did not set.
- A burst is refused for one service but not the other with identical keys : compare the
  keys' own limits before the workspace default.

_Last verified 2026-11-06 against the live host (master acdaffc)._
