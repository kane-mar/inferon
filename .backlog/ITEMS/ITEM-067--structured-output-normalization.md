## ITEM-067: Structured-output normalization (guard on the default provider)

**As a** tenant using the default DeepSeek provider with a JSON-schema contract
**I want** the schema guard to actually engage
**So that** cheap-tier structured output is validated on the flagship path, not
only when I bring an OpenAI-style provider

**Finding (2026-10-13):** the real DeepSeek API rejects
`response_format: {"type": "json_schema"}` with `400 This response_format type is
unavailable now`, and accepts `{"type": "json_object"}` (verified live).
`guard::is_guarded_request` requires a **json_schema** contract, so on the default
provider the guard can never fire — a headline feature is dormant on the default
deployment.

**Decision (2026-10-13):** normalize at the proxy (option A) rather than only
document the limitation (option B). The guard reads the contract from the
**original** payload (`meta.raw_prompt`), so only the **wire format** we send
upstream needs to change; the guard keeps the real schema and validates the cheap
response against it. The provider guarantees "valid JSON", the guard guarantees
"matches your schema".

**Acceptance Criteria:**
1. A capability decision `supports_json_schema(kind, model)` — `deepseek` (and the
   `deepseek-*` families) → **false**; `openai` → **true**; `openai-compatible`
   → **unknown, leave the request as sent** (never downgrade a provider that may
   support json_schema).
2. When a request carries a `json_schema` contract and the target does not support
   it, the **forwarded body** is rewritten to `{"type": "json_object"}`; the
   original payload (and therefore guard engagement + the schema used for
   validation) is unchanged.
3. The request rewrite is per-attempt: a fallback to a provider that *does*
   support json_schema keeps the original contract (the chain is resolved per
   model, so the capability is evaluated per route).
4. **The fallback replay is validated too.** Today `fallback_to_frontier` serves
   the replay without re-checking the schema; with `json_object` the frontier can
   return valid JSON that violates the contract. The replay must be validated
   against the same schema and, on violation, answered with a clear typed error
   (or recorded as a `schema_violation`) — never silently served.
5. A trace makes the degradation visible: when the wire format was downgraded,
   the guard event records it (e.g. `wire_format: "json_object"`), so the
   dashboard distinguishes "validated" from "validated after provider downgrade".
6. `json_object`-only requests (no schema) keep today's behaviour: not guarded,
   passed through.

**Technical Notes:**
- `budget::apply_model_override` reserializes the **original** payload (with
  `json_schema`), so the normalization must be applied when building the upstream
  body — a small transform after the override, before `.send()`.
- `openai-compatible` gateways are the hard case: default to pass-through and
  consider an adaptive retry (on a `400` mentioning `response_format`, retry once
  with `json_object`) as a follow-up rather than guessing up front.
- The DeepSeek capability check belongs with the provider kind (ITEM-043/050),
  next to `is_anthropic` / `auth_headers`.

**Source:** 2026-10-13 — schema-guard finding surfaced while live-verifying the
`is_cheap_tier` fix (commit `4e2dd88`). See the daily log entry of the same date.
