-- ITEM-058: savings attribution.
--
-- `traces.savings_usd` records what a request saved by being served on a cheaper
-- model than the client asked for (a router downgrade or a fallback), and
-- `traces.savings_kind` says which (`router` | `fallback`). Cache savings are
-- already on `traces.cache_savings_usd` (ITEM-053).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS savings_usd NUMERIC(10, 6) NOT NULL DEFAULT 0;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS savings_kind TEXT;
