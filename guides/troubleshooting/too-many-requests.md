# 429: our limit, or the provider's

A `429` is always "slow down". Which one is slowing you down decides the fix, and the
two look different in the body and in the Quality report.

## Symptom

Your client gets `429 Too Many Requests`. One of these two bodies comes back:

```json
{"error":{"message":"requests per minute exceeded for this key","type":"rate_limited"}}
```

```json
{"error":{"message":"<the provider's own message>","type":"rate_limit_error"}}
```

## What it means

**`rate_limited` is ours.** The key you used hit its per-minute request or token cap,
or the workspace default it inherits. The request never reached a provider and cost
nothing. The answer carries the window:

```text
retry-after: 53
x-ratelimit-limit: 1
x-ratelimit-remaining: 0
x-ratelimit-reset: 53
```

**`upstream_429` is the provider's.** Your limits were fine, and the provider refused
the call. The body is the provider's own, forwarded unchanged, and it carries no
`x-ratelimit-*` headers from us. The request did reach the provider, so it may count
against your provider bill.

## Fix

For **ours**:

1. Read `x-ratelimit-limit`: that is the cap that fired (requests or tokens per minute).
2. Raise it on **Settings → API keys** (`Requests per minute`, `Tokens per minute`), or
   change the workspace default on **Routing → Controls**. `0` means no limit.
3. Or spread the load: give each service its own key, so one burst cannot starve the
   other.
4. Wait out `Retry-After` before retrying; retrying earlier just extends the wait.

For **the provider's**:

1. Open **Routing → Fallback chain** and add a step whose **Handles** includes `429`.
   The next step runs instead of returning the refusal.
2. Or back off for the time the provider gives you and retry.
3. If it repeats at normal volume, the provider's plan is the limit, not Inferon.

## How to confirm it worked

- **Ours:** a request after `Retry-After` returns `200`, and **Quality → Errors by
  cause** records no new `rate_limited` rows.
- **Provider's, with a fallback step:** the client gets `200`, and the trace shows the
  step that served it:

```json
"routing_event": {"step": 1, "reason": "upstream_429", "to_model": "deepseek-flash", "from_model": "deepseek-v4-pro"}
```

- Both: **Quality → Errors by cause** names the class verbatim (`rate_limited` or
  `upstream_429`), so the count tells you which one is still happening.

Both kinds of `429` carry `x-inferon-trace-id`, and each names its own row: ours is
traced as `rate_limited`, and the provider's as `upstream_429`. The class in **Errors by
cause** is the row's class, so the header and the counter agree.

## When to stop and ask

- A `429` arrives when the key has `0` (no limit), the workspace default is `0`, and
  there are no `x-ratelimit-*` headers. That is the provider's, so treat it as theirs.
- `Retry-After` is far beyond a minute, or the same key is limited on its first request
  in a fresh window.
- The provider `429`s at a volume you have been running for weeks.
- A fallback step handles `429` and the client still receives `429`, with **Routing →
  Recent changes** showing the chain you expect.

_Last verified 2026-11-05 against the live host (master 3da7bd3)._
