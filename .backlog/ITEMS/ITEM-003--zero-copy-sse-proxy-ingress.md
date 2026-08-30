## ITEM-003: Zero-Copy SSE Proxy Ingress

**As a** developer using the OpenAI or Anthropic SDK
**I want** to point my baseURL at `https://api.inferon.dev/v1` and keep streaming exactly as before
**So that** I get zero added latency and zero code changes

**Acceptance Criteria:**
1. `POST /v1/chat/completions` with `stream: true` pipes upstream bytes to the client as `text/event-stream` without buffering the body (Axum Body → `reqwest::Body::wrap_stream` → Axum response)
2. `stream: false` returns the full JSON response with `usage` passthrough (10MB body limit)
3. Added latency on the streaming path is < 1ms (zero-copy; no in-memory buffering that would break TTFT)
4. Default upstream is DeepSeek (`deepseek-chat`) using the master upstream key from env — OpenAI SDK schema compatible
5. Upstream failure → HTTP 502 with error body; non-2xx upstream status is forwarded
6. TTFT measured at first upstream chunk (available to telemetry, ITEM-005)

**Technical Notes:**
- Stack: axum 0.7, reqwest 0.12 (stream/json), tokio 1.38, tower 0.4 (from implementation spec)
- Model passthrough only for now — model mutation is Slice 2 (see `budget-constrained-routing.md`)
- `async_stream` macro for the tapped stream; keep `[DONE]` chunks flowing untouched
