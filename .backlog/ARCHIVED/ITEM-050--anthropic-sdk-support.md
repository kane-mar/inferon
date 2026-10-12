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

## Close-out (2026-10-12)

- `anthropic` provider kind + `POST /v1/messages` ingress; a `Surface` guard
  refuses to cross surfaces (typed 400).
- `x-api-key` + `anthropic-version` auth (overridable via provider metadata); the
  request body is forwarded unchanged; usage is parsed from both JSON and SSE
  (`message_start` input / `message_delta` output, `cache_read_input_tokens`);
  `system` (string or text blocks) is hashed like an OpenAI system prompt.
- The heuristic router and schema guard are OpenAI-shaped and **explicitly
  bypassed**; the hard budget cap still applies (`budget::check_cap`).
- Claude price rows added (sonnet/haiku/opus) — no default-row overcharge.
- The Providers page offers the kind with a default base URL; the pre-add probe
  and model discovery are kind-aware.
- Tests: Rust **226 → 236**, clippy clean; tsc/eslint/i18n green. Commit `659caf8`,
  CI run `38246243129` (test+web+deploy) green.
- **Live**: `kinds` includes `anthropic`; a draft probe reaches the real Anthropic
  API and relays its 401; `/v1/messages` with no Anthropic provider → typed 400
  `no_provider`; add/remove of the kind works; and a temporary Anthropic provider
  pointed at the real API forwarded `POST /v1/messages` end to end, relaying
  Anthropic's `401 authentication_error` verbatim with a trace
  (`claude-3-5-sonnet-20241022`, 401, provider attributed).
- **PO gate**: a real Anthropic key is needed to exercise the 2xx/usage path live
  (same shape as the GitHub OAuth gate); the mock integration tests cover it.
