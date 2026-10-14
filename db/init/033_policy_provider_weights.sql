-- ITEM-069: provider health weights in the adaptive policy.
--
-- `tenant_policies.provider_weights` holds the per-provider health weights the
-- loop derives from telemetry (error rate + p95), keyed by provider id. Empty =
-- no opinion (the static provider ordering applies).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_policies
    ADD COLUMN IF NOT EXISTS provider_weights JSONB NOT NULL DEFAULT '{}'::jsonb;
