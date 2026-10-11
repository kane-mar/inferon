-- ITEM-051: persist a provider's last connectivity check so the Providers page
-- can show at-a-glance reachability (set by the Test-connection probe).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE providers ADD COLUMN IF NOT EXISTS last_check_at TIMESTAMPTZ;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS last_check_ok BOOLEAN;
