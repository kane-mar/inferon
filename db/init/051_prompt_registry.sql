-- ITEM-097: name a prompt, see its versions.
--
-- Identity stays content-derived (the existing `system_prompt_hash`): a label is
-- additive metadata on `(tenant, name, hash)`, so unmanaged prompts keep working
-- and a label can be attached retroactively. Deleting a label never deletes
-- traces, and there is no editing or deployment path in v1 (read-only registry).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS prompt_labels (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    prompt_name TEXT NOT NULL,
    system_prompt_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, prompt_name, system_prompt_hash)
);

CREATE INDEX IF NOT EXISTS idx_prompt_labels_hash
    ON prompt_labels (tenant_id, system_prompt_hash);
