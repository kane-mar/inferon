-- ITEM-047: dashboard spend breakdowns & limits.
--
-- Attribute each trace to the provider and API key that served it (both NULL
-- when the request ran on the shared platform key / a legacy credential), and
-- allow per-key / per-provider spend limits (0 = no limit) that the alert
-- worker evaluates by reusing the ITEM-020 alert episodes.

ALTER TABLE traces ADD COLUMN IF NOT EXISTS provider_id UUID;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS api_key_id UUID;

CREATE INDEX IF NOT EXISTS idx_traces_tenant_provider
    ON traces(tenant_id, provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_traces_tenant_key
    ON traces(tenant_id, api_key_id, created_at DESC);

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS spend_limit_cents BIGINT NOT NULL DEFAULT 0;
ALTER TABLE providers
    ADD COLUMN IF NOT EXISTS spend_limit_cents BIGINT NOT NULL DEFAULT 0;

-- Per-entity alert classes (`key_spend_limit:<uuid>`) exceed the original width.
ALTER TABLE alerts ALTER COLUMN class TYPE TEXT;
