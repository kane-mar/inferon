-- Providers gain an explicit default: the provider the proxy uses for a model
-- that no provider kind owns (e.g. an `openai-compatible` catch-all). Exactly
-- one default per tenant.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE providers ADD COLUMN IF NOT EXISTS is_default BOOLEAN NOT NULL DEFAULT FALSE;

-- Backfill: each tenant's first provider becomes its default (the de-facto
-- proxy fallback before this column existed).
UPDATE providers p
SET is_default = TRUE
WHERE p.is_default = FALSE
  AND p.id = (
      SELECT p2.id FROM providers p2
      WHERE p2.tenant_id = p.tenant_id
      ORDER BY p2.created_at
      LIMIT 1
  )
  AND NOT EXISTS (
      SELECT 1 FROM providers p3
      WHERE p3.tenant_id = p.tenant_id AND p3.is_default
  );

CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_one_default
    ON providers(tenant_id) WHERE is_default;
