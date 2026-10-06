-- ITEM-043: tenant-provisioned LLM providers (bring your own credentials).
--
-- A tenant may register one or more providers; a request resolves to the
-- tenant's provider (base URL + key) and falls back to the shared platform key
-- when none is configured. Provider keys are stored encrypted at rest (XChaCha20-
-- Poly1305, keyed by PROVIDER_ENCRYPTION_KEY) — `key_ciphertext` never leaves
-- the backend and is never returned; `key_hint` (last 4 chars) is for display.
CREATE TABLE IF NOT EXISTS providers (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    key_ciphertext TEXT NOT NULL,
    key_hint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_providers_tenant ON providers(tenant_id, created_at);
