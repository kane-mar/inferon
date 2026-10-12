-- ITEM-054: record the heuristic router's downshift on the trace
-- (`{from_model, to_model, reason:"heuristic_router"}`), so the saving is
-- observable rather than only visible as a different served model.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS router_event JSONB;
