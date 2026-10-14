-- ITEM-066: proxy overhead split.
--
-- `traces.proxy_ms` is the proxy's own pre-upstream time: request-received →
-- upstream dispatch (auth, parse, routing, budget, affinity, policy, provider
-- resolution). Together with `time_to_first_token_ms` and `total_duration_ms`
-- it splits a request into proxy overhead, upstream wait, and generation.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS proxy_ms INT;
