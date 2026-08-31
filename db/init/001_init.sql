-- Inferon schema migration 001 (idempotent)
-- Source: inferon-complete-implementation-spec.md (schema.sql), adapted to project decisions

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    api_key_hash VARCHAR(64) UNIQUE NOT NULL,
    upstream_openai_key TEXT,
    upstream_anthropic_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS traces (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    request_model VARCHAR(64) NOT NULL,
    system_prompt_hash VARCHAR(64),
    prompt_tokens INT NOT NULL DEFAULT 0,
    completion_tokens INT NOT NULL DEFAULT 0,
    time_to_first_token_ms INT,
    total_duration_ms INT NOT NULL,
    upstream_cost_usd NUMERIC(10, 6) NOT NULL DEFAULT 0.000000,
    waste_detected JSONB,
    status_code INT NOT NULL,
    raw_prompt JSONB,
    raw_completion JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_traces_tenant_created ON traces(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_traces_prompt_hash ON traces(system_prompt_hash);
