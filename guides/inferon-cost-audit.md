# The Inferon cost audit: what we can *show* before we pitch

This is the value-first document. It describes the concrete, honest thing Inferon can
pull out of a company's AI traffic and hand back as a free, personal cost audit
*before* anyone asks for a sale. Every finding maps to a field Inferon already
records (see `src/linter.rs`, `src/cost.rs`, `src/telemetry.rs`). Nothing here is
aspirational or invented; it is the data the proxy already keeps, written up as a
leak report for a customer.

---

## The guiding idea

A cold message should not say "here is my product", and it should not even say "let
me save you money". It should name **one specific leak** the person half suspects
but cannot see. The report opens the door; the product comes later.

The most important line to say to a founder:

> "You do not have a cost problem you can see yet. You have a cost problem you
> cannot *explain*. Here is the exact line item you did not know about."

---

## Finding 1: A model that is too big for the job

**What Inferon detects** (linter rule A): a frontier model that has a cheaper
partner configured : `gpt-4o` → `gpt-4o-mini`, `deepseek-v4-pro` →
`deepseek-flash` : answering calls with `prompt_tokens < 100` **and**
`completion_tokens < 50`.

**What we tell them:**
> "N% of your requests go to GPT-4o with fewer than 100 input and 50 output tokens.
> Those are classification and extraction calls that `gpt-4o-mini` handles with no
> measurable loss in quality. At your volume that is **$X/month you could keep**."

**Why it lands:** it names a *specific number* from *their* traffic, not a vague
"you could optimise". "GPT-4o is overkill" is a feeling; a dollar figure tied to a
request type is a decision.

---

## Finding 2: A frontier model used on plain text

**What Inferon detects** (linter rule C): a frontier model with a cheaper partner
used for simple unstructured text : no tools, functions or `response_format`, plain
string messages under about 3,000 characters.

**What we tell them:**
> "A big share of your traffic is plain text : no tools, no schema, no structured
> output , and it still goes to the frontier model. That is exactly the job
> `gpt-4o-mini` (or an open 8B model) is built for. Moving it cuts that part of the
> bill by about **90%**."

**Why it lands:** they realise it after the fact , "I was using a sports car to drive
to the corner shop" , and we show them before the bill does.

---

## Finding 3: A big system prompt that is never cached

**What Inferon detects** (linter rule B): the same system prompt (same hash)
repeating **2× or more** at **4,000 prompt tokens or more**, with
`cached_prompt_tokens == 0`. That means the provider charges for the whole opening
text on every single call.

**What we tell them:**
> "You send the same roughly 20k-token system prompt on every request, and it is
> *not* being cached, so every user pays for the full instruction set again. Prompt
> caching (or trimming the prompt) saves up to **$Y/month**, and that is pure
> saving with no effect on quality."

**Why it lands:** it is the cost that grows quietly with scale. Most founders do not
know caching exists. Naming it gives them the "oh, *that* is why" moment.

---

## Finding 4: Savings the router already captured (our strongest finding)

**What Inferon computes** (`savings_usd`, `savings_kind`, `router_event`,
`cache_affinity`): the estimated saving from routing a request down to a cheaper
model, or from keeping a cached prompt warm. In other words, what a smart proxy
recovers and a plain pass-through cannot.

**What we tell them:**
> "Over 30 days, X% of your calls were simple enough to send to a cheaper model
> safely : **$Z/month you are leaving on the table**, with no loss in quality,
> because nothing on your request path is looking."

**Why it lands, and why competitors cannot copy it easily:** a pure observability
tool (Langfuse, Helicone, Datadog) can *show* the spend but cannot compute this,
because it does not sit in the routing path. LiteLLM does not produce it either (its
router follows configuration, it does not classify cost). **This is the gap our
competitor research found.**

---

## Finding 5: What actually caused the bill (model, provider, key)

**What Inferon computes** (dashboard `summary`, `by_model`, `by_provider`,
`by_prompt`, and the fields on each trace): real spend split by model, provider and
API key. It also reports error rates and p50/p95 time to first token.

**What we tell them:**
> "Your surprise $400 is not one number. It is this: $X on gpt-4o, $Y on deepseek,
> $Z from *one specific key or workspace*. Here is exactly who and what drove it."

**Why it lands:** it turns bill shock ("I stared at the dashboard for a minute") into
an explanation ("*that* is the call eating my margin"). This is the event that opens
the conversation for Findings 1–4.

---

## Finding 6: Quality and failures (trust, not only cost)

**What Inferon computes** (`outcome`, `error_class`, `truncated`, quality thumbs):
whether a response was `ok`, `empty`, `refused`, `error` or `truncated`, and why a
request failed (`upstream_429`, `client_abort`, `timeout`, `guard`, …).

**What we tell them:**
> "You are paying for requests that fail quietly : X% error rate, or streams that
> were cut off from your cheap tier. That is why 'cheaper' felt unreliable. Here is
> exactly which failures come from the provider and which are yours."

**Why it lands:** it answers the objection before they raise it. The founders who
insist "cheap models are too flaky" are the ones this finding turns around: the
flakiness was a visible, fixable problem, not a property of cheap models. This is
the bridge from "cut my cost" to "trust the router".

---

## How this works in practice

The cost audit is not a document you *send*. It is a **menu of six named findings**,
each one sentence long, tied to their traffic and to a dollar figure. The
value-first approach:

1. **Look at their real traffic** : they point their SDK at Inferon, or give us a
   read-only window; the proxy already collects this as a drop-in.
2. **Name the one finding that hurts most** : usually Finding 5 first (the bill
   explainer), then 1, 2 or 3.
3. **Lead with that number, not with the product.**

This is the "free $100 resource" from Step 3 of the validation framework. The audit
is genuinely useful, it needs Inferon's place on the request path, and it is given
away *before* the sale.

## The honest limit (do not overclaim)

Findings 1–3 need **real traffic through the proxy**. They cannot be worked out from
a bill screenshot or a guess. So the value-first offer is really an invitation:
"let me run your traffic and show you the leak". That is fine : it *is* the Step 3
minimum test, and sending traffic through a drop-in proxy is a small step (one base
URL plus one key). But do not promise a dollar figure before you have seen the
traffic.
