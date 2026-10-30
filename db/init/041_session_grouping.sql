-- ITEM-093: group requests into a client transaction.
--
-- The client may send `X-Inferon-Session-Id`; the proxy stores it on the trace
-- (tenant-scoped) so cost, turns and errors can be reported per transaction.
-- Absent = the request is its own group, as before.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS session_id TEXT;

CREATE INDEX IF NOT EXISTS idx_traces_session
    ON traces (tenant_id, session_id, created_at DESC);
