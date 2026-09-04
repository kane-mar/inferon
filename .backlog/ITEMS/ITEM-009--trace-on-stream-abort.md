## ITEM-009: Trace Emitted When a Client Aborts a Stream

**As an** operator
**I want** a trace even when the client disconnects mid-stream
**So that** telemetry doesn't silently lose the request (and its partial tokens)

**Acceptance Criteria:**
1. Client disconnects after N chunks → a trace is still written with the tokens parsed so far (prompt + partial completion), the truncated state flagged
2. Normal stream completion still emits exactly one trace (no duplicates)
3. The hot path stays non-blocking (the abort path uses a synchronous send, never `.await`)
4. Upstream stream errors still emit the trace

**Technical Notes:**
- Current gap: the emit runs after the `async_stream` loop, so dropping the stream future (client abort) skips it — see `src/lib.rs::stream_with_trace`
- Design hint: a guard struct whose `Drop` runs `try_send` synchronously when the normal emit hasn't fired; flag the trace (e.g. `status_code` stays the forwarded status, `waste_detected`-style JSONB flag or raw_completion marker) so dashboards can distinguish truncated traces
- Hard to reproduce in unit tests: drive the response body stream and drop it after the first chunk, then assert the rx received a trace (mock upstream with many small chunks)
