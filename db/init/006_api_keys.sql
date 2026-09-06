-- Inferon schema migration 006 (idempotent)
-- ITEM-034: API keys become first-class rows. One tenant may hold several
-- revocable keys; only SHA-256 hashes are stored. Existing tenants.api_key_hash
-- rows are backfilled as each tenant's first key, then the retired column is
-- dropped (auth resolves through api_keys from here on).

CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT 'default',
    key_hash VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_tenant ON api_keys(tenant_id);

-- Backfill: each existing tenant's single key becomes its first api_keys row
-- (same hash — existing keys keep working, per ITEM-034 AC1). Gated on the
-- retired column still existing so a re-apply (idempotency) skips the
-- backfill once the column is gone.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.columns
               WHERE table_name = 'tenants' AND column_name = 'api_key_hash') THEN
        INSERT INTO api_keys (tenant_id, label, key_hash)
        SELECT id, 'migrated', api_key_hash FROM tenants WHERE api_key_hash IS NOT NULL
        ON CONFLICT (key_hash) DO NOTHING;
    END IF;
END $$;

-- Auth resolves through api_keys from here on; the column is retired.
ALTER TABLE tenants DROP COLUMN IF EXISTS api_key_hash;
