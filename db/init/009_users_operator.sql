-- Inferon schema migration 009 (idempotent)
-- ITEM-037: platform-operator flag on users. Bootstrap = env
-- OPERATOR_GITHUB_LOGINS (comma-separated GitHub logins), applied at sign-in
-- and backfilled at boot (promote_logins); a future promote API flips the
-- column too. Operator sessions scope to the platform-wide views.

ALTER TABLE users ADD COLUMN IF NOT EXISTS is_operator BOOLEAN NOT NULL DEFAULT false;
