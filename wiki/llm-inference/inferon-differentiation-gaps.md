# Inferon Differentiation: Claims vs. Current State (2026-10-30)

> Source: [2026-10-30 competitive positioning brief](../../raw/llm-inference/2026-10-30-competitive-positioning.md)
> Updated: 2026-10-30
> Purpose: hold the product claim and the code-grounded reality side by side, so
> the backlog closes real gaps rather than assumed ones.

## The claim

Inferon's defensibility is **not** routing or cost charts (commodities). It is
closing the loop between production consumption and private asset generation:

1. **Runtime**: a single Rust binary with sub-millisecond wire latency, suitable
   for agentic loops of 10–20 calls per transaction.
2. **Pre-flight cost linting**: inspect payload bloat *before* it leaves the
   perimeter, and prune/downgrade in flight.
3. **Passive distillation**: turn verified production traffic into deduplicated
   JSONL by task signature, then route to a distilled adapter.

## Measured current state (live, 2026-10-30)

Evidence window: last 7 days on the durable host — **775 traces, 2 tenants,
6 models**. The sample is dominated by one ~800k-token workload, so latency below
distinguishes small requests from huge bodies.

| Dimension | Measured today |
|---|---|
| Prompt storage | **774 / 774** traces have `raw_prompt` |
| Completion storage | **12 / 774** have `raw_completion` (non-streaming only; both stream paths store `None`) |
| Outcome signal | 768 `ok`; guard validation fired **0** times in the window |
| Proxy overhead (`proxy_ms`, prompt < 1000 tokens, n=5) | p50 **12 ms**, p95 **35 ms** |
| Proxy overhead (800k-token bodies) | **1.9–2.5 s** (dominated by receiving + JSON-parsing the body) |
| Hot-path store reads | ~10 per request: policy, route/provider, model-enabled, budget, affinity, fallbacks, guard |
| Config caching | none — every request re-reads the tenant config |
| Task grouping | exact `system_prompt_hash` only; the ONNX embedder (ITEM-028/079) has **no consumer** |
| Dataset export | none; `audit::report` is a read-only cost report |

## Gap matrix

Each row is a claim from the brief, what exists, and the backlog item that closes
it. Items are ranked in `BACKLOG.md`; the order below is by pillar.

### Pillar 1 — Passive distillation ("the true missing link")

| Claim | Current state | Gap item |
|---|---|---|
| Capture input→output pairs on the wire | Prompts stored; streamed completions dropped | **ITEM-085** |
| "2,000 verified examples" for a prompt | `outcome` taxonomy + thumbs + guard exist; nothing counts or qualifies examples per prompt | **ITEM-086** |
| "Click here to download the fine-tuning JSONL" | No export, no sample counts, no spend-per-prompt view | **ITEM-087** |
| "Store everything on the wire" (implicit) | Raw prompts stored for every trace; no switch, no retention window, no purge | **ITEM-088** |
| Grouped by "task signature" | Exact hash only; embedder unused | **ITEM-089** |
| "Route this prompt directly to your distilled adapter" | `Trainer`/`Gate`/`Promoter` seams only (ITEM-076); no adapter serving (ITEM-032) | *existing ITEM-076 / ITEM-032* |

### Pillar 2 — Pre-flight cost linting

| Claim | Current state | Gap item |
|---|---|---|
| Flag redundant system prompts across turns | Post-hoc linter rule B (≥4000 tokens, ≥2 repeats) + cache-hit rate + `keep_warm` at request time | **ITEM-090** |
| Detect schema bloat / token-hungry boilerplate | Complexity score only (tools/JSON/length/markers) | **ITEM-090** |
| Dynamically prune context | Nothing prunes or rewrites messages | **ITEM-091** |
| Downgrade simple classify/extract traffic | ✅ delivered: `plan_models` (complexity → tenant tier pairs → policy → budget zone) | — |

### Pillar 3 — Runtime and the unit of work

| Claim | Current state | Gap item |
|---|---|---|
| "Sub-millisecond wire latency" | p50 12 ms / p95 35 ms for small requests; ~10 store reads, no cache | **ITEM-092** |
| "10–20 calls per user transaction" is the economic unit | Per-request traces only; no session/transaction grouping | **ITEM-093** |

### Commodity parity (deliberately below the pillars)

| Claim (competitor) | Current state | Gap item |
|---|---|---|
| Format translation to 100+ backends (LiteLLM) | `surface_mismatch` refuses cross-surface; Anthropic SDK cannot reach DeepSeek/OpenAI | **ITEM-094** |
| Rate limiting across business units (Portkey) | Spend caps only; no RPM/TPM | **ITEM-095** |
| PII masking (Portkey) | None; raw payloads stored | **ITEM-096** |
| Prompt-version tracking (Langfuse) | Content hash only, no versions/diff | **ITEM-097** |
| Async eval (Langfuse) | Outcomes + thumbs, no gold sets/runs | **ITEM-098** |
| Latency heatmaps (Langfuse) | Time series + p50/p95/p99, no heatmap | **ITEM-099** |
| Simple load balancing (LiteLLM) | Health-weighted best-provider pick; no distribution | **ITEM-100** |

## What already exceeds the brief

- **Savings attribution that reconciles**: router/fallback/cache savings against
  the unreduced frontier cost, with cache-hit-aware pricing.
- **Budget as a three-state breaker**: allow → downgrade in the last 15% → hard 429,
  plus per-key/per-provider caps and alerts.
- **Adaptive policy**: telemetry-derived threshold and provider weights, versioned,
  auditable and reversible.
- **On-prem reality**: keyless `local` provider, $0 traces with counterfactual
  attribution, reachability sweep, per-tenant tier pairs, self-hosted install.
- **Interception, not observation**: guarded responses are validated and replayed,
  not just logged.

## The five things between today and the pitch

1. Persist streamed completions (ITEM-085) — without it there is no dataset.
2. A verified-example filter and per-prompt counts (ITEM-086).
3. A dataset surface + JSONL export with provenance (ITEM-087), behind a retention
   and consent switch (ITEM-088).
4. Semantic task signatures (ITEM-089) — wire the embedder that already exists.
5. A config cache with a measured latency budget (ITEM-092) if "sub-millisecond" is
   to be claimed honestly; today the overhead is DB round trips, not Rust.

## See Also

- [Customer-Hosted LLMs](customer-hosted-llms.md) — the three connectivity profiles.
- [Passive LoRA Distillation Pipeline](passive-lora-distillation.md) — the original
  design this brief sharpens.
- [Inferon Project Direction](inferon-project-direction.md) — governing decisions.
