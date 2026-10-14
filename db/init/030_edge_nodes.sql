-- ITEM-065: edge node enrollment (hybrid data plane slice 2, cloud half).
--
-- An edge node runs the proxy next to a customer's own model; the cloud keeps
-- the control plane. `edge_nodes` holds one revocable credential per node
-- (only its hash). `last_seen_at` is the node's health; revocation is a row
-- delete. The provider key never appears here — it stays local to the edge.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

CREATE TABLE IF NOT EXISTS edge_nodes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash VARCHAR(64) NOT NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_edge_nodes_token ON edge_nodes(token_hash);
CREATE INDEX IF NOT EXISTS idx_edge_nodes_tenant ON edge_nodes(tenant_id);
