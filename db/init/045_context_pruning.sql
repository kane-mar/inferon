-- ITEM-091: opt-in context pruning.
--
-- The policy rides the item-064 versioned row (`tenant_policies.prune`): NULL
-- means pruning is off, which is the default for every tenant.
--
-- `traces.prune_event` attributes a rewrite to the policy that made it: what was
-- removed, the bytes/tokens/USD saved and the original payload hash. Held-out
-- requests carry the event too, with `held_out` true and nothing removed, so the
-- holdout comparison can be computed from one column.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_policies ADD COLUMN IF NOT EXISTS prune JSONB;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS prune_event JSONB;
