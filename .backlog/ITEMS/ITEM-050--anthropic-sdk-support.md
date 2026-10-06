## ITEM-050: Native Anthropic SDK support (provider adapter)

**As a** developer using the Anthropic SDK
**I want** to point my client's `base_url` at Inferon and have Anthropic Messages
traffic proxied, metered, and cost-guarded
**So that** Claude users get the same cost-leak telemetry and protection as
OpenAI users

**Context (re-scoped out of ITEM-043, PO 2026-10-06):** ITEM-043 provisions
"bring your own provider" for the OpenAI-compatible kinds
(`deepseek | openai | openai-compatible`). Native Anthropic is **not**
OpenAI-shaped:

- ingress is `POST /v1/messages` (not `/v1/chat/completions`);
- auth is `x-api-key` + `anthropic-version` (not `Authorization: Bearer`);
- the body splits `system` out of `messages`, requires `max_tokens`, and uses a
  different tool/stop shape;
- responses and SSE use a different envelope, and usage is
  `input_tokens`/`output_tokens`.

The proxy, router, schema guard, and `cost::price_for` are all OpenAI-shaped, so
this is a new adapter rather than a loose end. The product wiki already frames
Inferon as usable from "your existing OpenAI or Anthropic SDK".

**Acceptance Criteria:**
1. A tenant can add an `anthropic` provider kind; the proxy serves the Anthropic
   Messages API surface (`POST /v1/messages`) with the tenant's provider key.
2. Requests and non-streaming responses are translated/forwarded correctly
   (auth header swap, `system` handling, `max_tokens`), with the client's SSE
   passed through or translated for streaming.
3. Token usage from Anthropic responses is recorded on traces (prompt/completion
   tokens, TTFT, duration).
4. Anthropic models are priced in `cost::price_for` (no default-row overcharge).
5. The schema guard, heuristic router, and fallback chain either understand
   Anthropic traffic or are explicitly bypassed for it (no OpenAI model ids sent
   to an Anthropic endpoint).

**Technical Notes:**
- Add an ingress route + a small adapter seam keyed by provider `kind`; reuse the
  `ProviderStore`/`Route` resolution (ITEM-043) and the telemetry worker.
- Anthropic can be reached either by a native Anthropic client (forward the
  Anthropic-shaped request) or by translating an OpenAI-shaped request; decide
  which surface(s) to support first.
- `x-api-key` replaces the `Authorization` swap; never log key material.

**Source:** ITEM-043 AC1 re-scope (PO 2026-10-06) — native Anthropic SDK support.
