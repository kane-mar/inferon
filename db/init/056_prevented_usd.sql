-- ITEM-140: money the protections stopped, recorded where the decision was made.
--
-- "Prevented" is the third number on the overview beside spent and saved: what
-- the request would have cost had it run. Three sources, one column pair:
--   budget      -- refused at the monthly cap
--   rate_limit  -- refused by a key's RPM/TPM limit
--   downgrade   -- the 85% zone served it on the cheap tier instead
--
-- It is an estimate (prompt tokens by the shared estimator, completion from the
-- caller's max_tokens or the documented default), and the interface says so. A
-- nullable-free default of 0 keeps every existing row meaningful: those requests
-- were not prevented.
ALTER TABLE traces ADD COLUMN IF NOT EXISTS prevented_usd NUMERIC(20, 10) NOT NULL DEFAULT 0;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS prevented_kind TEXT;
