-- ITEM-045: ordered fallback chains. A tenant's chain is an ordered list of
-- model ids tried when the primary request fails retryably (transport error,
-- timeout, 429, 5xx). The absence of a row (or an empty list) means "no chain"
-- — behaviour is exactly as before.
CREATE TABLE IF NOT EXISTS routing_policies (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    fallbacks JSONB NOT NULL DEFAULT '[]'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The served model already lands in `request_model`; this records *why* it
-- changed (from/to + reason) so the dashboard can explain a fallback.
ALTER TABLE traces ADD COLUMN IF NOT EXISTS routing_event JSONB;
