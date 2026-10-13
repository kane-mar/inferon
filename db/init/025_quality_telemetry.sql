-- ITEM-055: response-quality telemetry.
--
-- `traces.outcome` records whether a request produced usable content (`ok`),
-- nothing usable (`empty`), a refusal/content-filter stop (`refused`), an
-- upstream/proxy error (`error`), or ended early (`truncated`) — derived by the
-- proxy, not the client.
--
-- `trace_quality` carries the optional out-of-band client signal (thumbs), keyed
-- by trace id. It is a separate table (not a `traces` column) because the client
-- posts the signal after the response and the trace itself is written
-- asynchronously — an upsert here must never race the trace insert.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE traces ADD COLUMN IF NOT EXISTS outcome TEXT;

CREATE TABLE IF NOT EXISTS trace_quality (
    trace_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    signal TEXT NOT NULL,
    score DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS trace_quality_tenant_idx
    ON trace_quality (tenant_id, created_at DESC);
