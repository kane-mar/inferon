-- ITEM-006 follow-up (2026-10-05): re-price historical `deepseek-flash` traces.
--
-- DeepSeek's API returns the model as `deepseek-flash`, which matched neither
-- `deepseek-chat` nor `deepseek-v4-flash` in the price table (src/cost.rs), so
-- it fell through to the default row ($3.00 prompt / $15.00 completion per 1M)
-- instead of v4-flash ($0.22 / $0.66) — a ~13.7x overcharge on stored cost.
-- The code fix prices future traces correctly; this re-prices the rows already
-- written, at the v4-flash miss rate (per-trace cache hits were not persisted,
-- so this is a conservative upper bound).
--
-- Guard: only rows whose stored cost still equals the OLD default-row pricing
-- are touched. Once re-priced they no longer match, so re-applying this file on
-- every deploy is a no-op, and nothing priced by the fixed code is clobbered.
UPDATE traces
SET upstream_cost_usd = ROUND(
        (prompt_tokens::numeric / 1000000) * 0.22
      + (completion_tokens::numeric / 1000000) * 0.66,
        6
    )
WHERE request_model = 'deepseek-flash'
  AND ABS(
        upstream_cost_usd
        - ((prompt_tokens::numeric / 1000000) * 3.00
           + (completion_tokens::numeric / 1000000) * 15.00)
      ) < 0.000001;
