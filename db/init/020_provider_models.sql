-- Advertised models per provider, discovered from the host's `GET <base>/models`
-- (PO: "the models shouldn't be assumed to be openai; grab the available models
-- from the llm host"). The cache lets the proxy route a requested model to the
-- provider that advertises it without a static catalog.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS provider_models (
    provider_id UUID NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (provider_id, model_id)
);

-- Hot path: find the provider that advertises a model for a tenant.
CREATE INDEX IF NOT EXISTS idx_provider_models_model ON provider_models(model_id);
