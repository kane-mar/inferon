-- ITEM-057: error taxonomy & truncation visibility.
--
-- `traces.error_class` records why a failed or aborted request failed
-- (upstream_429 / upstream_5xx / client_4xx / inferon_4xx / transport /
-- timeout / guard / client_abort), chosen at the failure site. NULL for
-- successful requests.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS error_class TEXT;

CREATE INDEX IF NOT EXISTS traces_error_class_idx
    ON traces (error_class, created_at DESC)
    WHERE error_class IS NOT NULL;
