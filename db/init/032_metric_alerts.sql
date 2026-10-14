-- ITEM-068: metric-regression alert thresholds.
--
-- `tenant_configurations` already carries the spend/waste alert thresholds;
-- these add the error-rate and latency-regression thresholds (ITEM-068). They
-- are conservative by default so a new tenant is not spammed.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_error_rate NUMERIC(5, 4) NOT NULL DEFAULT 0.5;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_error_min_requests INT NOT NULL DEFAULT 20;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_latency_multiplier NUMERIC(6, 2) NOT NULL DEFAULT 3.0;
