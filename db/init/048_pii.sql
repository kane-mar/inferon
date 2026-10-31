-- ITEM-096: PII detection and opt-in redaction.
--
-- Detection runs by default and records counts by class — never a matched value.
-- Redaction is per tenant and has two independent switches: storage (the stored
-- payload, so datasets and the audit share inherit it) and wire (what the
-- provider sees), the latter with a holdout so the effect on quality is
-- measurable.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS pii_detection BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS pii_redact_storage BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS pii_redact_wire BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE traces ADD COLUMN IF NOT EXISTS pii JSONB;

CREATE INDEX IF NOT EXISTS idx_traces_pii ON traces (tenant_id, created_at DESC)
    WHERE pii IS NOT NULL;
