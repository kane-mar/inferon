## ITEM-094: Cross-format translation (any SDK to any backend)

**As a** team using the Anthropic SDK with a DeepSeek/OpenAI-compatible backend (or
the reverse)
**I want** Inferon to translate the request and response shapes
**So that** changing model does not mean changing client code

**Claim (2026-10-30 brief, landscape).** LiteLLM "translates OpenAI-format calls to
100+ backends". Inferon currently refuses: `resolve_route` returns
`surface_mismatch` when the provider kind does not match the client surface.

**Current state (2026-10-30).** `/v1/chat/completions` serves OpenAI-shaped
providers; `/v1/messages` serves `anthropic` providers. An Anthropic SDK call cannot
use DeepSeek, and an OpenAI SDK call cannot use Claude. No translation layer exists.

**Gap.** No cross-surface capability — a real integration barrier versus LiteLLM,
even though the brief treats routing/translation as commodity.

**Acceptance Criteria**
1. `POST /v1/messages` can target an OpenAI-compatible provider: translate
   system/messages/content blocks/tools/streaming to chat completions, and translate
   the response and SSE frames back to the Anthropic shape (including usage mapping
   and stop reasons).
2. `POST /v1/chat/completions` can target an `anthropic` provider: the inverse.
3. Translation is **per provider capability**: a provider declares which surfaces it
   accepts; a mismatched request without translation enabled keeps today's typed
   error.
4. Streaming preserves delta order, usage and stop reason; a mid-stream failure maps
   to the client surface's error shape.
5. Traces record `surface_in`, `surface_out` and the provider kind; cost uses the
   provider's model price, not the client's surface.
6. Golden-file tests per direction: tools, multimodal content (text-only fallback
   documented), system blocks, stop reasons, streaming, errors.

**Technical Notes**
- Translation is a pure function module with a narrow seam
  (`translate_request(surface_in → surface_out)`), so it is unit-testable without a
  network.
- Do not translate silently when information would be lost (e.g. Anthropic content
  blocks with unsupported types): return a typed 400 naming the field.
- The guard and router remain OpenAI-shaped; define explicitly which side of the
  translation they run on.

**Ranking note.** Rank **10** of 22 · V4/E4 — the biggest adoption unlock (any SDK, any backend), but commodity parity with a heavy test surface.
---

**Delivered (2026-10-31).** `src/translate.rs` (requests + responses both ways:
system blocks, tool definitions/choices, tool_use ↔ tool_calls, tool_result ↔
role:tool, stop reasons, usage, `max_tokens` default, error bodies) and
`src/translate/sse.rs` (two line-buffered state machines plus `adapt`, which
wraps the upstream byte stream *before* the telemetry tap so the trace parses the
same frames the client receives; the input-token count OpenAI reports only at the
end is carried in the translated `message_delta`, and `usage_from_anthropic_sse_bytes`
now reads it there).

`Route.translate` is the seam: `resolve_route` picks the provider's native
endpoint and records the surface it speaks; `wire_body` translates the last step
before the wire (routing, guard, pruning and lint stay on the client's shape) and
`forward_buffered`/`stream_response` translate back. A block the target cannot
represent returns `surface_translation_unsupported` naming the field. The
`translate_surfaces` tenant switch (migration 046, default on) restores the old
typed refusal.

Verified: 19 unit tests (incl. frame splits, tool-call partials, mid-stream error
mapping) + 4 handler tests (both directions, translated stream with usage on the
trace, kill switch).
