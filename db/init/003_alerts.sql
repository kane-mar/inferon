-- Inferon schema migration 003 (idempotent)
-- ITEM-020: cost anomaly alerts — persisted alert episodes + per-tenant
-- alert thresholds on tenant_configurations.

-- One row per firing episode; a clear event is recorded on the same row
-- (status -> cleared, cleared_at set), keeping history queryable.
CREATE TABLE IF NOT EXISTS alerts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    class VARCHAR(32) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'firing',
    detail JSONB NOT NULL DEFAULT '{}',
    threshold JSONB NOT NULL DEFAULT '{}',
    first_fired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_fired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    cleared_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_alerts_tenant_class ON alerts(tenant_id, class, status);
CREATE INDEX IF NOT EXISTS idx_alerts_status_fired ON alerts(status, last_fired_at DESC);

-- Per-tenant alert thresholds (AC5): conservative defaults that do not fire
-- on normal dev traffic; tuned after ITEM-018 brings real traffic.
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_spend_multiplier NUMERIC(6, 2) NOT NULL DEFAULT 5.00;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_spend_floor_cents BIGINT NOT NULL DEFAULT 100;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS alert_waste_daily_threshold INTEGER NOT NULL DEFAULT 50;
