-- ITEM-046: expose the heuristic router's controls on `tenant_configurations`.
-- The router ships on by default (matching ITEM-015's behaviour); a tenant can
-- turn it off or move the complexity threshold where a gpt-4o-tier request is
-- downgraded to the cheap tier.
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS router_enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS router_threshold DOUBLE PRECISION NOT NULL DEFAULT 0.35;
