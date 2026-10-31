-- ITEM-095: velocity limits per API key.
--
-- `api_key_usage` is the authority for the counters: one row per key per
-- minute, incremented atomically with the check. `0` on a limit means unlimited
-- (today's behaviour), and a key with no limits never touches this table.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS rpm_limit BIGINT NOT NULL DEFAULT 0;
ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS tpm_limit BIGINT NOT NULL DEFAULT 0;
-- The tenant default, applied to keys that set no limit of their own.
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS rpm_limit BIGINT NOT NULL DEFAULT 0;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS tpm_limit BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS api_key_usage (
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    requests BIGINT NOT NULL DEFAULT 0,
    tokens BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (api_key_id, window_start)
);

CREATE INDEX IF NOT EXISTS idx_api_key_usage_window ON api_key_usage(window_start);
