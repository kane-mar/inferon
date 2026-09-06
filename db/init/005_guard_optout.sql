-- Inferon schema migration 005 (idempotent)
-- ITEM-027 AC1: per-tenant schema-guard opt-out. The guard is ON by default
-- for cheap-tier structured-output requests; a tenant can disable it here.
-- DEFAULT TRUE + the COALESCE in the reader mean an unconfigured tenant (no
-- row) is always guarded — the safety net ships without a migration scare.

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS guard_enabled BOOLEAN NOT NULL DEFAULT TRUE;
