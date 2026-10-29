-- ITEM-081: per-tenant router tier pairs.
--
-- Each entry is `{"frontier": "...", "cheap": "..."}`: the models the heuristic
-- router may downshift between for this tenant. Tenant pairs win over the
-- built-in table in `src/router.rs`; an empty array means the defaults only.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS tier_pairs JSONB NOT NULL DEFAULT '[]'::jsonb;
