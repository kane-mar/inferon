-- ITEM-079: read-only cost-audit shares.
--
-- A tenant mints a share token and can hand out the audit report without giving
-- anyone an account. Only the hash is stored; revoking is a timestamp, so an
-- audit trail survives.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS audit_shares (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT '',
    token_hash VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_viewed_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_audit_shares_token ON audit_shares(token_hash);
CREATE INDEX IF NOT EXISTS idx_audit_shares_tenant
    ON audit_shares(tenant_id, created_at DESC);
