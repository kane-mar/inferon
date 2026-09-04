## ITEM-010: Rule B Repeat Counts Survive Proxy Restarts

**As an** operator
**I want** the "repeating 4,000+ token uncached system prompt" rule to keep counting across proxy restarts
**So that** a restart doesn't reset detection of a recurring cost leak

**Acceptance Criteria:**
1. Occurrence counts per `system_prompt_hash` are persisted (DB table or Postgres-backed counter), not only in worker memory
2. On worker boot, counts are loaded so a prompt seen 5x before a restart is flagged on its next occurrence (repeat_count ≥ 2 semantics preserved)
3. The worker's hot loop is unchanged: batching/lint/cost still happen off the request path
4. Counting stays correct under concurrent flushes (single worker owns the counts)

**Technical Notes:**
- Current state: `telemetry_worker` keeps `hash_counts: HashMap<String, u32>` in memory only — resets on restart; rule B is in `src/linter.rs`
- The dashboard already surfaces repeated-prompt spend via `by-prompt`, so this item is about the *flag* surviving; simplest correct shape is a small `prompt_hash_counts(hash, updated_at)` table updated during the batch flush, or folding the count into the flush transaction
- Source: ITEM-006 gap note
