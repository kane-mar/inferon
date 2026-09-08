-- Inferon schema migration 010 (idempotent)
-- ITEM-039: tenant membership invites. No email infra exists (Rust + bash,
-- OAuth-only signup) — an owner names a GitHub login; when that user signs in
-- they join the tenant as `member` (claim-based). The invite row is consumed
-- at claim.

CREATE TABLE IF NOT EXISTS tenant_invites (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    github_login TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, github_login)
);
