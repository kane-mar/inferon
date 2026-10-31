-- ITEM-090: pre-dispatch payload findings.
--
-- `request_lint` is a small JSON object: the finding kinds that fired, an
-- estimated wasted-token count and its USD value. Advisory only — the payload
-- is forwarded unchanged. NULL = nothing fired.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS request_lint JSONB;
