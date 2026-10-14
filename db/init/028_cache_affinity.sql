-- ITEM-063: cache-affinity routing.
--
-- `prompt_affinity` remembers, per tenant and per system-prompt prefix, the
-- provider/model that most recently served that prefix from the provider's
-- cache. Routing consults it (TTL-bounded) to avoid trading a warm cache for a
-- marginally cheaper miss.
--
-- `traces.cache_affinity` records when affinity changed the serving model.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS prompt_affinity (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    system_prompt_hash TEXT NOT NULL,
    provider_id UUID,
    model TEXT NOT NULL,
    last_hit_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, system_prompt_hash)
);

ALTER TABLE traces ADD COLUMN IF NOT EXISTS cache_affinity JSONB;
