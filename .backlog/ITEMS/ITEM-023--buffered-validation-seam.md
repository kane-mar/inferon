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
