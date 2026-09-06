#!/usr/bin/env bash
# Mint a tenant (ITEM-011, ITEM-034): stores sha256(dev key) as the tenant's
# first api_keys row + the tenant's upstream master key, prints the
# developer-facing key. Only the hash is stored; one tenant may hold several
# revocable keys.
#
# Usage: seed-tenant.sh "<upstream master key>" [tenant-label]
# Needs a running Postgres (docker compose up -d postgres) and psql on PATH,
# or DATABASE_URL pointing at an Inferon database.
#
# Prerequisite check example (compose):
#   docker compose up -d postgres
#   ./scripts/seed-tenant.sh sk-my-master-key

set -euo pipefail

MASTER_KEY="${1:?usage: seed-tenant.sh <upstream master key> [tenant-label]}"
LABEL="${2:-default}"

# Developer key the tenant's engineers will present to the proxy.
DEV_KEY="sk-inferon-$(openssl rand -hex 12)"
HASH=$(printf '%s' "$DEV_KEY" | shasum -a 256 | awk '{print $1}')

psql_cmd() {
    if [ -n "${DATABASE_URL:-}" ]; then
        psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -tAc "$1"
    else
        docker exec inferon-postgres-1 psql -U inferon -d inferon -v ON_ERROR_STOP=1 -tAc "$1"
    fi
}

# Tenant + first key row in one statement (ITEM-034): tenants no longer
# carries the key hash — api_keys does. Each run mints a fresh tenant with a
# fresh random key (the pre-006 single-hash dedupe anchor is gone).
psql_cmd "WITH t AS (
              INSERT INTO tenants (upstream_openai_key) VALUES ('$MASTER_KEY')
              RETURNING id
          )
          INSERT INTO api_keys (tenant_id, label, key_hash)
          SELECT id, '$LABEL', '$HASH' FROM t;" >/dev/null

echo "tenant seeded (label: $LABEL)"
echo "developer API key: $DEV_KEY"
echo "  -> Authorization: Bearer $DEV_KEY"
echo "master key stored for upstream calls (never shown to developers)"
