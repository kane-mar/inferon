-- Inferon schema migration 004 (idempotent)
-- ITEM-025/026: schema guard event on the trace. Nullable — unguarded traces
-- carry NULL, so this telemetry is strictly additive (ITEM-026 AC4).

-- One JSONB guard event per guarded request, attached at flush time:
--   {"outcome": "fallback" | "fallback_failed", "reason": <GuardReason label>,
--    "guarded_model": <cheap model that fired the guard>, "detail": ...}
ALTER TABLE traces ADD COLUMN IF NOT EXISTS guard_event JSONB;

-- Guard-count queries (ITEM-026 views) filter on the reason label.
CREATE INDEX IF NOT EXISTS idx_traces_guard_reason
    ON traces ((guard_event ->> 'reason')) WHERE guard_event IS NOT NULL;
