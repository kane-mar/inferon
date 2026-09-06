## ITEM-026: Schema Guard — Guard Telemetry (Slice 4 · was 021d)

**As an** Inferon operator and tenant
**I want** to see when and why the guard fired
**So that** I can judge whether the cheap tier is trustworthy and feed failures back into training

**Acceptance Criteria:**
1. Every guard event records outcome + failure reason on the trace (schema migration only if the existing trace shape can't carry it)
2. Tenant dashboard and operator views surface guard/fallback counts per model and per failure reason over the window selector
3. Negative samples (prompts that triggered a fallback) are queryable via the existing system-prompt-hash shape — ready to feed a future DPO/distillation dataset without extra plumbing
4. Unguarded traces are never mutated — this telemetry is additive only

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021d
- Depends on ITEM-025 (events exist to record)
- The >5% fallback-rate drift signal that Slice 5's evolution loop consumes (022d) reads from this data

## Verification record (2026-09-06)

- **AC1 (every guard event carries outcome + failure reason)**: the guard now
  records all three verdicts as `guard_event` JSONB on the request's trace
  (column + partial index from migration 004, landed with ITEM-025):
  `{"outcome":"flushed","guarded_model":...}` on a clean cheap flush,
  `{"outcome":"fallback","reason":...}` / `fallback_failed` from the
  ITEM-025 circuit. The trace shape could not carry it before (raw_completion
  / waste_detected are semantically owned), hence the migration.
- **AC2 (dashboard + operator views, window selector)**: new
  `GET /v1/dashboard/api/guard?days=` (tenant) and
  `GET /v1/dashboard/api/operator/guard?days=` (operator) return guarded /
  fallback / fallback-failed totals plus breakdowns per guarded model and per
  failure reason over the shared window selector; both share one query shape
  scoped by `($1::uuid IS NULL OR tenant_id = $1)`. Auth gates mirror the
  other views (401/403/operator). Smoke asserts 7/30/90 windows on the guard
  view and the operator gate (401 no key, 403 tenant key, 200 operator key).
- **AC3 (negative samples by hash)**: fallbacking prompts are returned as
  `system_prompt_hash` clusters (`negative_samples`, hash shape unchanged),
  and the existing trace/prompt queries already surface their raw_prompts — no
  new plumbing for a future DPO/distillation harvest.
- **AC4 (additive only)**: unguarded traces never mutate — regression test on
  the passthrough path asserts `guard_event` stays None; the column is NULL
  for everything the guard never judged.
- Note: guard counts surface through the JSON APIs (same surface pattern the
  ITEM-020 alerts view uses); the dashboard shell page is unchanged. 123 tests
  green.
