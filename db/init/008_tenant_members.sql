-- Inferon schema migration 008 (idempotent)
-- ITEM-036: tenant membership. A user signs in (users, migration 007); a
-- first-time sign-in auto-provisions a tenant + owner membership (self-serve
-- signup). Memberships gate session-scoped dashboard access (ITEM-036 AC2) and
-- later tenant member management (ITEM-039) — role is owner|member.

CREATE TABLE IF NOT EXISTS tenant_members (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_tenant_members_user ON tenant_members(user_id);
