-- ITEM-090: controls for the pre-flight lint.
--
-- `lint_enabled` is the per-tenant kill switch: off means the pre-dispatch pass
-- is skipped entirely (the post-hoc linter is unaffected).
--
-- `lint_dismissed_at` records that the tenant judged a trace's finding a false
-- positive. Dismissed findings stay on the trace but stop counting as actionable.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS lint_enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE traces
    ADD COLUMN IF NOT EXISTS lint_dismissed_at TIMESTAMPTZ;
