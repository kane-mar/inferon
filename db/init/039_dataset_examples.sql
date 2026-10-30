-- ITEM-086: verified-example qualification for the dataset surface.
--
-- `example_state` is decided at trace-write time: 'qualified' or 'excluded'
-- (NULL when no completion was captured at all). `example_reason` names the
-- exclusion. `completion_hash` is the exact-dedupe identity of the completion.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS example_state TEXT;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS example_reason TEXT;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS completion_hash TEXT;

CREATE INDEX IF NOT EXISTS idx_traces_examples
    ON traces (tenant_id, example_state, created_at DESC);
