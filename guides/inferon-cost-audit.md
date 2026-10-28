# The Inferon Cost Audit — what we can *see* and *hand back* before any pitch

This is the "value-first" artifact: the concrete, honest thing Inferon can
extract from a founder's LLM traffic and hand to them as a free, personalized
cost audit — *before* asking for a sale. Every finding maps to a real field
Inferon already computes (see `src/linter.rs`, `src/cost.rs`, `src/telemetry.rs`
in this repo). Nothing here is aspirational or invented; it is the
telemetry the proxy already records, reworded as a customer-facing leak report.

---

## The framing principle

A cold opener should not be "here's my product" or even "let me save you money."
It should be **one concrete, specific, named leak** that the person already
half-suspects but can't see. The report is the wedge; the product is downstream.

The single most important line in the whole report, stated to a founder:

> "You don't have a cost problem you can see yet — you have a cost problem you
> can't *attribute*. Here's the exact line item you didn't know about."

---

## Finding 1 — Over-provisioning (the sledgehammer-for-a-thumbtack)

**What Inferon detects** (linter rule A): a frontier model that has a cheaper
tier pair configured — `gpt-4o` → `gpt-4o-mini`, `deepseek-v4-pro` →
`deepseek-flash` — handling `prompt_tokens < 100` **and**
`completion_tokens < 50`.

**The line we hand them:**
> "N% of your requests hit GPT-4o with fewer than 100 input / 50 output tokens.
> Those are classification and extraction calls a `gpt-4o-mini` handles with no
> measurable quality loss. At your volume that's **$X/mo you could keep**."

**Why it lands:** it names a *specific number* tied to *their* traffic, not a
generic "you could optimize." It converts "GPT-4o is overkill" (a vague feeling)
into a quantified, attributed line item.

---

## Finding 2 — Frontier model on simple text (model misuse)

**What Inferon detects** (linter rule C): a frontier model with a cheaper tier
pair used for simple unstructured text — no tools/functions/`response_format`,
plain-string messages under ~3,000 chars.

**The line we hand them:**
> "A big share of your traffic is plain text — no tools, no schema, no
> structured output — still hitting the frontier model. That's the exact
> workload `gpt-4o-mini` (or an open 8B) is built for. Moving it cuts that
> portion of the bill by **~90%**."

**Why it lands:** it's the founder's *retroactive* realization ("I was using a
Ferrari to drive to the corner store") — surfaced proactively instead of after
the bill shocks them.

---

## Finding 3 — The un-cached giant system prompt (context bloat)

**What Inferon detects** (linter rule B): the same system prompt (same hash)
repeating **≥2×** at **≥4,000 prompt tokens** with `cached_prompt_tokens == 0`
(i.e. the provider is re-billing the whole prefix every call).

**The line we hand them:**
> "You're re-sending the same ~20k-token system prompt on every single request,
> and it's *not* being cached — so every user re-pays the full instruction set.
> Prompt caching (or trimming) cuts that cost by up to **$Y/mo** and it's pure
> savings, no quality tradeoff."

**Why it lands:** this is the "death by a thousand tokens" that compounds with
scale. Most founders don't even know caching exists. Naming it is an instant
"oh, *that's* why" moment.

---

## Finding 4 — Money left on the table by the router (the moat finding)

**What Inferon computes** (`savings_usd`, `savings_kind`, `router_event`,
`cache_affinity`): the estimated saving from *routing a downgrade* or *cache
affinity* — i.e. what a smarter proxy would recover that a dumb passthrough
can't.

**The line we hand them:**
> "Across a 30-day window, X% of your calls were simple enough to safely route
> to a cheaper tier — **$Z/mo in savings you're currently leaving on the
> table**, with no quality loss, because nothing on your request path is
> looking."

**Why it lands — and why it's defensible:** a pure observability tool (Langfuse,
Helicone, Datadog) can *show* the spend but can't compute this, because it
doesn't sit in the routing path. This is the one finding LiteLLM won't produce
either (its router is config-fallback, not cost-classified). **This is the
unclaimed line in the market** our competitive research identified.

---

## Finding 5 — The bill-shock explainer (per-model / per-provider / per-key)

**What Inferon computes** (dashboard `summary`, `by_model`, `by_provider`,
`by_prompt`, telemetry attribution): true spend broken down by model, provider,
and API key — with error rates and p50/p95 TTFT.

**The line we hand them:**
> "Your $400 surprise isn't one number — it's this: $X on gpt-4o, $Y on
> deepseek, $Z from *one specific key/tenant*. Here's exactly who and what
> drove it."

**Why it lands:** it converts bill shock ("I stared at the dashboard for a
minute") into attribution ("*that's* the call eating my margin"). This is the
*entry event* that unlocks Findings 1–4.

---

## Finding 6 — The quality/error story (trust, not just cost)

**What Inferon computes** (`outcome`, `error_class`, `truncated`, quality
thumbs): whether responses were `ok / empty / refused / error / truncated`, and
why a failure occurred (`upstream_429`, `client_abort`, `timeout`, `guard`, …).

**The line we hand them:**
> "You're paying for requests that are silently failing — X% error rate /
  truncated streams from your cheap tier, which is why 'cheaper' felt
  unreliable. Here's exactly which failures are upstream vs yours."

**Why it lands:** it addresses the *counter-hypothesis* head-on. The founders
who insist "cheap models are too flaky" are the ones this finding converts —
you're showing them the flakiness was a *visible, fixable* problem, not a
property of cheapness. This is the trust bridge from "cut my cost" to "trust
the router."

---

## What this means operationally

The cost audit is not a document you *send*. It is a **menu of six named
findings**, each a single-sentence, attributed, dollar-tied claim. The value-
first opener is:

1. **Look at their actual traffic** (they point their SDK at Inferon, or grant
   a read-only window — your proxy already does this as a drop-in),
2. **Name the single most acute finding** for them (usually Finding 5 first —
   the shock explainer — then 1/2/3),
3. **Lead with that number, not with the product.**

This is the "free $100 resource" from Step 3 of the validation framework: the
audit is genuinely valuable, it requires Inferon's position on the request path,
and it's given away *before* the sale.

## The honest caveat (don't overclaim)

Findings 1–3 require **real traffic through the proxy** to be computed — they
are not derivable from a bill screenshot or a hunch. The value-first offer is
therefore inherently a **"let me run your traffic and show you the leak"**
invitation, not a cold "here's a number." That's fine — it *is* the Step 3
minimum-viable-test, and the act of running traffic through a drop-in proxy is
low-friction (one base URL + one key). But don't promise a dollar figure
before you've seen the traffic.
