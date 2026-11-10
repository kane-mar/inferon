-- ITEM-141: what the adaptive loop optimises for, per workspace.
--
-- The loop had one direction: cheaper whenever quality held. That is right for a
-- cost product and wrong for a workspace whose answers matter more than their
-- price. `objective` is the workspace's statement of intent; every step the loop
-- takes answers to it, and it bounds how low the threshold may go.
--
-- Existing rows default to 'balanced' — the behaviour they already had.
ALTER TABLE tenant_policies ADD COLUMN IF NOT EXISTS objective TEXT NOT NULL DEFAULT 'balanced';
