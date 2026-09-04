-- Inferon schema migration 002 (idempotent)
-- ITEM-009: truncated-stream marker on traces
-- ITEM-010: per-hash repeat counts for linter rule B (survive restarts)
-- ITEM-016: per-tenant budget configuration (integer cents per project decision)

ALTER TABLE traces ADD COLUMN IF NOT EXISTS truncated BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS system_prompt_counts (
    hash VARCHAR(64) PRIMARY KEY,
    count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tenant_configurations (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    budget_limit_cents BIGINT NOT NULL DEFAULT 0,
    current_spend_cents BIGINT NOT NULL DEFAULT 0,
    cheap_fallback_model TEXT
);
