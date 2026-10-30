-- ITEM-092: the proxy's own overhead at microsecond resolution.
--
-- `proxy_ms` is fine for a dashboard but cannot express a sub-millisecond
-- claim. `proxy_us` is the same measurement (request start -> before the first
-- upstream dispatch, so it excludes the upstream and the response body) in
-- microseconds. The performance suite reads its p50/p95.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS proxy_us BIGINT;
