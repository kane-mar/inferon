## ITEM-130: A stream that stops before its finish_reason reaches the client as a silent end

**As** a developer streaming from the gateway
**I want** every stream to end with a `finish_reason` or a named failure
**So that** a cut answer is something I can see and retry, not an exception with
no cause and no id

**Reported** 2026-11-07 (PO): `Error: Stream ended without finish_reason`.

**Delivered and verified live 2026-11-07** on the rebuilt proxy, through a
deliberately broken upstream on the compose network: `ends-early` answered a
content frame then closed → the client received the frame plus
`{"error":{"code":"stream_ended_early","message":"… (request id 051ea3e1-…)"}}` and
`[DONE]`, and the trace read `truncated=t, class=transport`; `empty-answer` → 
`empty_answer` with the trace `outcome=empty, truncated=f`; a real `deepseek-flash`
stream still ends on the provider's own `finish_reason` with nothing injected. The
temporary tenant, provider and mock container were removed afterwards.

**What was happening.** A client asking for `deepseek-flash` with `stream: true`
got a transient upstream 5xx; the fallback chain (ITEM-109) stepped to
`qwen/qwen3.8-27b` and **resolved it to the right provider** (`local-llm`, the
Mac Studio on the tailnet — it advertises exactly that model). After ~17 s the
local model's upstream body simply closed. The proxy forwarded it — the client
received SSE headers, one late chunk, no `finish_reason` and no error — so its SDK
raised a message that names nothing. Four such traces on the live host, `ttft`
equal to `total_duration_ms` to the millisecond (one chunk, at the very end), 0
tokens, no captured completion.

The first draft of this item said the chain stepped to "a model that provider does
not serve". That was wrong, and the trace is why: `provider_id` is stamped from the
*primary* route and never updated when a fallback serves, so the row named Deepseek
for a request that went to `local-llm` (ITEM-133). The correct story is a local
model that stalled and was cut — not a misroute.

Two defects, one fix:

1. **A cleanly closed stream with no terminal frame was forwarded as-is.** The
   trace recorded `200` with outcome `empty`, so the dashboard counted the request
   as fine and no alert could fire.
2. **An upstream transport error mid-stream sent nothing at all.** `stream_with_trace`
   broke out of the loop, emitted no frame and ended the body — the same silent
   end, from a worse cause.

**Acceptance Criteria**

1. A stream whose answer arrived but whose terminal frame did not is closed with a
   chunk carrying `finish_reason` (reusing the provider's own `id`/`model`), so a
   complete answer is never mistaken for a cut one.
2. A stream that ended before the provider was done carries a typed error naming
   the trace: `stream_ended_early` (content, no terminal frame), `upstream_stream_error`
   (transport failure), or `empty_answer` (the provider finished with nothing).
3. The error is written in the surface the client speaks: an OpenAI `error` frame
   for `/v1/chat/completions`, an Anthropic `event: error` for `/v1/messages`.
4. Such a stream is recorded `truncated` with class `transport`, so Quality and the
   error-rate alert see it. An `empty_answer` stays `empty` (already a tracked
   outcome) and is not truncated.
5. The guarded flush path (ITEM-023) gets the same tail: a validated buffer that
   carries content but no `finish_reason` still reaches a client that cannot close it.
6. The upstream's bytes are unchanged on every normal stream — this is a tail, not
   a rewrite (ITEM-127 still holds).

**Technical Notes**

- The decision is one pure function, `sse::tail(raw, errored, surface, trace_id,
  model)`, with the surface passed from the handler (`StreamTap`), because the
  bytes the client reads are already in its own shape after translation (ITEM-094).
- Tests: seven unit tests over the five endings, plus two end-to-end tests
  (`ends-early`, `empty-answer`) asserting the client's bytes and the trace.
- The `SSE_BODY` and `delta_only_stream` fixtures gained their `finish_reason`:
  they were unrealistic (a provider's last chunk carries one), and they now keep
  the byte-identity tests honest about the normal path.
- **Still open, deliberately:** the empty stream is not retried. Nothing has
  reached the client when it happens, so a retry on the next chain step would be
  legal and would have hidden this whole failure — that is a bigger change to
  `dispatch_chain` (it needs a buffered first-byte window) and belongs in its own
  item, not in a fix for a live break.
- **Found while verifying, not fixed:** the chain accepted a step whose model the
  serving provider does not offer. Validation covers an empty set, a duplicate
  model and an unknown trigger (ITEM-109) but not "this provider cannot serve this
  model". A step like that turns a 5xx into a guaranteed bad fallback.

**Source:** PO report, 2026-11-07 ("the gateway has been failing with a message:
Error: Stream ended without finish_reason").
