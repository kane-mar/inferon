-- ITEM-089: semantic task signatures.
--
-- `task_signatures` stores the embedder's vector once per distinct prompt, so a
-- re-cluster never re-embeds and ITEM-029 can read the same pipeline. The label
-- is the representative prompt's hash, which keeps a cluster's identity stable
-- as new prompts arrive.
--
-- On the trace we keep the cluster label and where it came from: `embedding`
-- when the cluster is semantic, `hash` when there is no embedder and grouping
-- degraded to the exact hash (ITEM-089 AC4).
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS task_signatures (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    prompt_hash TEXT NOT NULL,
    signature_version TEXT NOT NULL,
    embed_model TEXT NOT NULL,
    dim INT NOT NULL,
    source TEXT NOT NULL,
    vector REAL[] NOT NULL,
    cluster_label TEXT NOT NULL,
    cluster_source TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, prompt_hash, signature_version)
);

ALTER TABLE traces ADD COLUMN IF NOT EXISTS task_cluster TEXT;
ALTER TABLE traces ADD COLUMN IF NOT EXISTS task_cluster_source TEXT;

CREATE INDEX IF NOT EXISTS idx_traces_task_cluster
    ON traces (tenant_id, task_cluster, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_traces_unclustered
    ON traces (tenant_id, created_at DESC)
    WHERE task_cluster IS NULL;
