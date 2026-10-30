-- ITEM-085: mark a captured completion that hit the size cap.
--
-- `raw_completion` now also holds streamed completions (assembled from SSE
-- deltas). A completion longer than the cap is stored as a head; this flag says
-- so, so the dataset surface never treats a capped capture as a full pair.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces
    ADD COLUMN IF NOT EXISTS raw_completion_capped BOOLEAN NOT NULL DEFAULT FALSE;
