-- ITEM-108: append-only audit of owner-only control-plane changes.
-- One row per changed setting, so the routing page can say who changed a
-- budget/guard/router/fallback/tier-pair/prune control and when. `actor_login`
-- is a snapshot: the event stays readable if the user is later removed.
CREATE TABLE IF NOT EXISTS config_audit (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_login TEXT NOT NULL,
    setting TEXT NOT NULL,
    before_value JSONB,
    after_value JSONB,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_config_audit_tenant_time
    ON config_audit (tenant_id, recorded_at DESC);
