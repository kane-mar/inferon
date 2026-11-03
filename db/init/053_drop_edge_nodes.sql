-- The edge node profile was retired on 2026-11-01.
--
-- It could not be a controller: prompts never left the node, so datasets, eval
-- and policy adaptation were impossible there, and its budget/limits were a
-- stale snapshot of other traffic. The full package (`--profile self-hosted`)
-- does everything it did, inside the customer's own network; the reachable
-- endpoint (`local` provider) covers calling an on-prem model from the cloud.
--
-- Existing installs may still have the table; new ones never create it.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

DROP TABLE IF EXISTS edge_nodes;
