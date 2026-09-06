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
