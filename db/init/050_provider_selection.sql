-- ITEM-100: why a provider was chosen.
--
-- `only` = there was one eligible provider (the pre-ITEM-100 behaviour),
-- `weight` = distributed by the adaptive policy's health weights,
-- `round_robin` = distributed evenly.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS provider_selection TEXT;
