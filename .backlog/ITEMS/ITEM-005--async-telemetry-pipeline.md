## ITEM-005: Async Telemetry Pipeline

**As a** operator
**I want** every request traced asynchronously with zero added latency on the hot path
**So that** I can report cost/usage and feed the cost linter (ITEM-006)

**Acceptance Criteria:**
1. On request completion, a `TraceMessage` (tenant_id, request_model, system_prompt_hash, token counts, ttft_ms, total_duration_ms, status_code, raw_prompt, raw_completion) is sent via `tokio::sync::mpsc` channel(10_000)
2. A dedicated Tokio background worker flushes to the `traces` table every 500ms **or** every 100 items, whichever comes first — DB I/O never blocks the hot HTTP path
3. system_prompt_hash is the SHA-256 of the first `role: system` message content
4. Token counts: non-stream from `usage`; streaming path records 0 and is flagged for the background token counter (known gap, see notes)
5. Channel-full behavior never blocks the request path (drop with error log or explicit backpressure policy)
6. Batch inserts are transactional (single DB transaction per flush)

**Technical Notes:**
- Source: implementation spec telemetry (see wiki `inferon-slice-engineering-spec.md` → Slice 1; `aio-proxy-overview.md` → Implementation Latency & Waste Rules)
- Known gap from the slice review: stream-mode writes `prompt_tokens: 0, completion_tokens: 0` ("inferred later by background token counter") — resolve or accept before marking done
