-- ITEM-088: consent and retention for captured payloads.
--
-- `payload_capture` gates whether raw prompts/completions (and therefore the
-- dataset pairs) are stored at all; costs, outcomes and aggregates are always
-- kept. `payload_retention_days` bounds how long raw payloads live (0 = keep
-- forever).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS payload_capture BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS payload_retention_days INTEGER NOT NULL DEFAULT 30;
