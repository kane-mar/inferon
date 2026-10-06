-- ITEM-044: per-tenant model selection. A row records that a tenant has an
-- opinion about a model; the absence of a row means "unrestricted" (allowed),
-- so unknown/own `openai-compatible` ids need no rows. At most one row per
-- (tenant, provider kind) has is_default — enforced in the store's set_default.
CREATE TABLE IF NOT EXISTS tenant_models (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, model_id)
);
