-- ITEM-090: which pre-flight rules a tenant wants.
--
-- An array of rule names from `preflight::ALL_RULES`. NULL means "all of them"
-- (the pre-052 behaviour), so existing rows keep scanning everything; an empty
-- array is a valid choice ("scan, but flag nothing").
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS lint_rules TEXT[];
