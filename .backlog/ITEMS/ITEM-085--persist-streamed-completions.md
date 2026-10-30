## ITEM-085: Persist streamed completions (the dataset's missing leg)

**As a** tenant whose traffic is mostly streaming
**I want** Inferon to store the completion my client actually received
**So that** the proxy can produce instruction pairs from real traffic, not just
from the occasional non-streaming call

**Claim (2026-10-30 brief).** "Because Inferon sits directly on the wire, it can
passively inspect schema-validated completions … it auto-indexes these
input-output pairs into clean, deduplicated datasets." Today the prompts are on
the wire but the outputs are not: **774/774** stored prompts vs **12/774** stored
completions in the last 7 days, because both streaming paths set
`raw_completion: None`.

**Current state (2026-10-30).** `forward_buffered` stores the response JSON;
`stream_response` and `guarded_stream_response` store nothing, even though the
SSE tap already parses every frame for usage/outcome.

**Gap.** No completion text for streamed traffic — which is the dominant mode.

**Acceptance Criteria**
1. For a streaming request, the assistant content the client received is stored as
   `raw_completion` in the same `{choices:[{message:…}]}` shape as the
   non-streaming path, assembled from the SSE deltas the tap already sees.
2. Capture is bounded: a per-completion byte cap (default 256 KB) stores a marked
   head rather than an unbounded body, and the trace records that it was capped
   (additive column, e.g. `raw_completion_truncated`).
3. An aborted or mid-stream-failed response stores exactly what was delivered and
   keeps the existing `truncated: true` marker; a failed request stores no pair.
4. Tool-call deltas (`tool_calls` / function arguments) are captured too, so a
   tool-using response is still a candidate example.
5. Zero-copy remains zero-copy for the client: the tap buffers only when capture is
   enabled, and the response bytes are still forwarded as they arrive.
6. Tests: single-frame, multi-frame, tool-call, client-abort, size-cap; cost and
   outcome unchanged.

**Technical Notes**
- Extend the existing tap that computes usage/outcome — do not add a second parser.
- The guard path already buffers content end-to-end; it can hand its buffer to the
  same capture helper.
- Capture follows the tenant's capture switch (ITEM-088); the switch defaults to
  on for parity with today's raw-prompt storage.

**Ranking note.** Rank **1** of 22 · V5/E1 — the smallest change that unblocks every distillation item — without it the edge cannot be demonstrated.