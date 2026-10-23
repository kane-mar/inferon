-- ITEM-031 slice 1: the evolution job queue.
--
-- Drift in a served model queues a job; the loop then trains, tests and swaps an
-- adapter. Only the queue/observability half is live today — the training and
-- hot-swap columns exist so later slices record their transitions without a
-- migration. At most one *open* job per (tenant, adapter) is enforced by a
-- partial unique index, so a drift pass is idempotent.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS evolution_jobs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    -- The served model whose guarded answers drifted (the "adapter").
    adapter TEXT NOT NULL,
    reason TEXT NOT NULL,
    -- queued | training | testing | promoting | promoted | failed
    state TEXT NOT NULL DEFAULT 'queued',
    evidence JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_evolution_jobs_tenant_created
    ON evolution_jobs(tenant_id, created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_evolution_jobs_open
    ON evolution_jobs(tenant_id, adapter)
    WHERE state IN ('queued', 'training', 'testing', 'promoting');
