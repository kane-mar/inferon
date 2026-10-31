## ITEM-091: Opt-in context pruning and payload rewriting

**As a** tenant with runaway prompt growth
**I want** Inferon to remove provably redundant payload before it is sent
**So that** I save tokens without a code refactor — and can prove I did not change
the answer

**Claim (2026-10-30 brief).** "Dynamically pruning context or downgrading simple
classify/extract payloads to lightweight local/cheap endpoints." The downgrade half
is delivered; the pruning half is not.

**Current state (2026-10-30).** The proxy rewrites the *model* (router, tier pairs,
budget zone) but never the *messages*: no truncation, no dedupe, no context budget.

**Gap.** No payload rewriting, and therefore no evidence that rewriting is safe.

**Acceptance Criteria**
1. **Off by default**, per-tenant opt-in with an explicit policy (refined at pull;
   candidates: drop exact-duplicate messages, cap repeated few-shot blocks, drop
   stale turns beyond a token budget). Tool definitions and the last user turn are
   never touched.
2. Every rewrite is traced: what was removed (hash/kind), tokens saved, bytes saved,
   and the original payload hash — so the change is attributable after the fact.
3. A **holdout** (e.g. 5–10%) is sent unpruned and compared on outcomes, thumbs and
   guard results; the dashboard shows the delta. A regression automatically turns
   the policy off (kill switch) rather than continuing to save money.
4. The estimated saving uses the same cost engine; the item reports measured, not
   projected, savings in its close-out.
5. A typed client error is never produced by pruning; a rewrite failure forwards the
   original payload.
6. Per-tenant and per-policy rollback (the policy is versioned like ITEM-064).

**Technical Notes**
- This is the riskiest item in the differentiation set: it changes the answer, not
  just the price. It lands only after ITEM-090 shows which payloads are actually
  redundant.
- Never prune a request with a schema contract without validating that the pruned
  payload still satisfies it.
- Reuse the policy versioning/audit surface (ITEM-064) rather than inventing a
  second config mechanism.

**Depends on** ITEM-090 (evidence), ITEM-064 (versioned policy), ITEM-092 (cache).
**Ranking note.** Rank **9** of 22 · V4/E4 — high savings, highest risk — it changes answers; only after 090 proves which payloads are redundant, and with versioned rollback.
---

**Delivered (2026-10-31).** `src/prune.rs`: `PrunePolicy` (default **off**),
`rewrite` (exact duplicate turns; a token budget removes the oldest stale turns;
system prompts, tool definitions and the final turn are protected), `in_holdout`
(deterministic on tenant + prompt hash, clamped at 50%), `Rewrite::event` (kinds,
turns, bytes, tokens and USD through the cost engine, plus the original payload
hash) and `evaluate` (24 h, 20-request floor per arm, 5-point regression margin →
publishes a version with `prune.enabled = false` and logs the rates).

The policy rides `tenant_policies.prune`, so freeze/rollback/version are the
ITEM-064 surface rather than a second config mechanism. The hot path reads it once
with the rest of the policy, applies it after routing, and records the event on
the trace; a rewrite failure or an already-minimal payload forwards the original.

Verified: 13 unit tests (rules, protection, budget order, holdout distribution,
event pricing, kill-switch decision, policy composition) + 4 handler tests
(rewritten trace, held-out control, schema-contract request untouched,
config-flag kill switch). Not yet enabled for a tenant: the item's own gate was
ITEM-090 evidence, which only starts accumulating now.

**Enablement (added 2026-10-31).** `POST /v1/dashboard/api/prune` (owner session)
publishes `{enabled, drop_duplicate_turns, max_prompt_tokens, holdout_percent}` as
a new `tenant_policies` version, preserving the router threshold/weights and
recording the replaced version in the evidence; `GET /v1/dashboard/api/config`
returns the active `prune`. Opting out publishes `enabled: false` (history kept),
and the operator freeze/rollback endpoints work unchanged.

**Two safety rules worth knowing when choosing a policy:** holdout is clamped at
50%, and a request carrying a JSON-schema contract is never rewritten (the guard
validates that answer, so its input is left as written).
