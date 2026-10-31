-- ITEM-098: gold sets, eval runs and the regression gate.
--
-- A gold set is immutable once pinned: the exact inputs (and the reference
-- answers) are copied out of `traces`, so later retention or pruning cannot
-- change what a run measured.
--
-- Eval calls are real provider calls but **never** written to `traces`: they are
-- recorded only here, so production aggregates, routing and the tenant's budget
-- are untouched by construction.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS eval_gold_sets (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    cases BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS eval_gold_cases (
    gold_set_id UUID NOT NULL REFERENCES eval_gold_sets(id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    prompt_hash TEXT,
    source_model TEXT NOT NULL,
    request JSONB NOT NULL,
    reference JSONB,
    PRIMARY KEY (gold_set_id, trace_id)
);

CREATE TABLE IF NOT EXISTS eval_runs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    gold_set_id UUID NOT NULL REFERENCES eval_gold_sets(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    status TEXT NOT NULL,
    max_cases BIGINT NOT NULL,
    max_cost_cents BIGINT NOT NULL,
    cases BIGINT NOT NULL DEFAULT 0,
    scored BIGINT NOT NULL DEFAULT 0,
    passed BIGINT NOT NULL DEFAULT 0,
    failed BIGINT NOT NULL DEFAULT 0,
    cost_usd NUMERIC(12, 6) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS eval_cases (
    run_id UUID NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    status TEXT NOT NULL,
    method TEXT,
    score NUMERIC(4, 3),
    latency_ms INT,
    cost_usd NUMERIC(12, 6) NOT NULL DEFAULT 0,
    detail TEXT,
    PRIMARY KEY (run_id, trace_id)
);

CREATE INDEX IF NOT EXISTS idx_eval_runs_tenant ON eval_runs (tenant_id, created_at DESC);
