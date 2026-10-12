-- ITEM-053: prompt-cache visibility. Persist the cached prompt-token count and
-- the estimated saving versus a no-cache call, so the dashboard can show the
-- cache hit rate and savings (real traffic is ~90% cached — the biggest cost
-- lever, previously invisible).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS cached_prompt_tokens INTEGER;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS cache_savings_usd NUMERIC(10, 6) NOT NULL DEFAULT 0;
