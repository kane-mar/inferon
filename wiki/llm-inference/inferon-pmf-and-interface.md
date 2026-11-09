# Where Inferon wins, and what the interface should therefore show

_Research note, 2026-11-09. Written from product knowledge, not from a live scrape of
each vendor's site — every claim below is worth re-checking against current pricing
pages before it goes into a sales conversation._

## The field, and what is now commodity

| Product | Shape | Has | Notable for |
|---|---|---|---|
| **LiteLLM** (BerriAI) | OSS proxy + enterprise | 100+ providers, virtual keys, spend tracking, router with fallbacks/retries/load balancing, budgets, caching | The default open-source gateway. Everything it does is table stakes now |
| **OpenRouter** | Marketplace | 300+ models, credits, provider preference, BYOK, price arbitrage, uptime routing | Distribution, not control: it is a shop, not a policy engine |
| **Portkey** | Gateway + observability | guardrails, prompt management, caching, routing, logs | Enterprise gateway, closed core |
| **Helicone** | Proxy observability | cost/latency/sessions/cache analytics, now gateway features | Started as "see your spend", drifted into gateway |
| **Langfuse / LangSmith** | Observability + eval | traces, prompts, datasets, evals | Where teams already look at answer quality |
| **TensorZero** | Rust gateway + flywheel | observability, evals, **fine-tuning from production data**, MIT, funded | **The closest competitor to Inferon's stated flywheel** |
| **Cloudflare AI Gateway**, **Vercel AI Gateway** | Infra-bundled | caching, logs, rate limits, credits, unified API | Cheap, sticky, sold with the platform |
| **Martian / Not Diamond** | Router-as-a-service | quality-based model selection | Closed; sells routing as a black box |

**Commodity set** (assume every competitor has it or will within a year):
multi-provider proxy, fallbacks and retries, caching, rate limits, virtual keys,
per-model spend dashboards, prompt management, evals, latency charts.

Inferon's claimed flywheel — production traffic → datasets → adaptation — is *not*
unique: TensorZero is building exactly that, with more maturity and funding. Treat it
as the second act, not the pitch.

## The four things that are actually defensible

1. **The bill is true, and it is computed in-band.**
   Inferon costs every request at the trace: provider list price, cache hits, prune
   savings, router/fallback delta. Most tools *estimate* cost after the fact from
   token counts, or wait for a provider's billing export. "Our number matches your
   invoice" is a claim a dashboard that estimates cannot make.

2. **Cost protection is a control loop, not a limit.**
   At 85% of budget it downgrades to the cheap tier; at 100% it refuses with a typed,
   traceable error; per key RPM/TPM; every move attributed. Competitors implement
   *caps* (refuse above X). A cap harms the product; a downgrade keeps it running and
   saves the money. This is the most differentiated thing in the product today.

3. **A local model is first class and priced at zero in the same ledger.**
   "Cheap tier = your own GPU" only means something when the gateway can cost it
   (zero), route to it, verify its answers, and show the delta next to cloud models.
   Cloud-only gateways cannot express it; BYO-endpoint products treat it as
   reachability, not economics.

4. **Honesty as a visible stance.**
   Claims carry their basis: gateway time is quoted with its payload size, the
   measurement's start is stated, excluded traffic is counted on the page. Every
   dashboard in this market shows a number without its basis. In a market of
   optimistic vendor claims, a product that says "8 ms at 1.2 MB, here is what is
   excluded" is unusual enough to be a differentiator — and it is cheap to keep.

## What this means for the interface

The buying trigger for a cost product is **"our AI bill jumped and nobody knows
why"**. The user's job is: *see the bill → see what was avoidable → act → see it did
not come back*. The interface should be organised around that loop, not around the
gateway's internals.

Ranked gaps against that loop:

1. **The operator cannot see or configure adaptation.** The adaptive policy (provider
   weights, router threshold, version, evidence) exists in the backend with operator
   read/freeze/rollback endpoints — and no UI reaches any of it. Worse, nothing lets
   an operator *set* policy, only freeze it. The "progressively inspecting and
   adapting" capability is invisible and unconfigurable. **ITEM-139.**
2. **The dashboard does not lead with the value case.** Overview opens with requests,
   spend, error rate, p95, savings. The hero should be *spent vs saved vs prevented*,
   with the saving attributed (router · fallback · cache · prune · local) and the
   protection counted (downgrades, refusals, rate-limit stops). A customer renews on
   "you were saved $X", not on a p95. **ITEM-140.**
3. **Runtime is an internal claim surface in the tenant's nav.** Gateway
   microbenchmarks (and the band/drift analysis built for them) serve the honesty
   stance and the operator — not a customer looking at their bill. It should live
   under Operator, leaving the tenant's seven tabs about money, quality and data.
4. **No first-value moment.** After the first requests, the most persuasive screen in
   the product is "you would have spent $X more without Inferon, here is how" — and
   it does not exist as a moment, only as a number in a grid.
5. **The cost audit is the PMF report, and it is buried.** Six findings, each with a
   dollar figure and a control, is exactly the "what was avoidable" step. It is a
   separate page reached from a tab called Cost audit; it should feed Overview's
   attention strip (it partly does) and be the landing surface for a new workspace
   with traffic.

## What I would *not* add

- More explanation on screen (ITEM-138 settled that: a link, not a paragraph).
- A model catalogue page to browse 300 models. The product's claim is that the
  customer should not have to choose; a catalogue contradicts it.
- Charts without a decision attached. The drift grid earns its place because it
  distinguishes "slower" from "bigger prompts"; a pretty line per metric would not.

**Next:** ITEM-139 (operator adaptation surface: see, set, freeze, roll back, with
the evidence), then ITEM-140 (value-first Overview, Runtime demoted to Operator).
