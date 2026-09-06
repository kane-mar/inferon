## ITEM-023: Schema Guard — Buffered Validation Seam (Slice 4 · was 021a)

**As a** developer running cheaper models in production
**I want** guarded responses held until they can be validated
**So that** an invalid cheap-model response never reaches my client

**Acceptance Criteria:**
1. For guarded requests only (carries `response_format.json_schema.schema` **and** routed to a cheap tier), streamed SSE chunks are buffered in memory instead of piped straight through; the buffer flushes to the client only after `[DONE]` + successful validation
2. Unguarded traffic keeps the existing zero-copy passthrough — byte-for-byte unchanged
3. A mid-stream client abort during the buffered phase still emits the ITEM-009 truncated trace (no dangling connection)

**Technical Notes:**
- Parent: ITEM-021 (archived) — this is its first slice, in pull order
- Reuses/extents the buffering introduced for trace-on-abort (ITEM-009) rather than adding a second mechanism
- Design decision pending (from parent): spec behavior is buffer-then-validate at `[DONE]`; incremental streaming validation is an optimization that trades TTFT flatness for complexity — decide before/at this slice

## Verification record (2026-09-06)

- **Decision (from parent, applied)**: spec behavior — buffer-then-validate at
  `[DONE]`; no incremental streaming validation (keeps one validation path).
- **AC1 (guarded = schema contract + cheap tier; buffered flush)**: requests
  with `response_format.json_schema.schema` served by a cheap-tier model
  (gpt-4o-mini / deepseek flash family) take a fully buffered path in
  `proxy_chat_completions`; the flush gate (`guard::check_guarded_sse`,
  container integrity this slice) runs before anything reaches the client.
  Test: guarded valid stream flushes whole after [DONE]; guarded truncated
  upstream (frames, no [DONE]) is rejected 502, not forwarded.
- **AC2 (unguarded unchanged)**: zero-copy passthrough is untouched — a
  truncated body WITHOUT a schema contract pipes straight through
  byte-for-byte (regression test); frontier model with schema contract is not
  guarded. 106 tests green.
- **AC3 (mid-buffer abort -> truncated trace)**: `BufferedGuard` Drop hook
  (same pattern as ITEM-009's StreamEmit) emits exactly one truncated trace
  when the client disconnects during the buffered phase.
- Schema-level conformance (jsonschema over `choices[0].message.content`) is
  ITEM-024's slice and slots into `check_guarded_sse` next.
