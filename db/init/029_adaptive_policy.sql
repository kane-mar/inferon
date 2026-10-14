-- ITEM-064: adaptive policy loop — versioned, auditable, reversible policy.
--
-- `tenant_policies` is append-only: each analysis pass publishes a new version
-- with the evidence that justified it. `enabled` is the kill switch (all rows
-- false = static config). `traces.policy_version` attributes an outcome to the
-- policy that was active, so a bad policy is visible and can be rolled back.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS tenant_policies (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    router_threshold DOUBLE PRECISION,
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, version)
);

ALTER TABLE traces ADD COLUMN IF NOT EXISTS policy_version BIGINT;
