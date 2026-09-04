#!/usr/bin/env bash
# Mint a tenant (ITEM-011): stores sha256(dev key) + the tenant's upstream master
# key, prints the developer-facing key.
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

psql_cmd "INSERT INTO tenants (api_key_hash, upstream_openai_key)
          VALUES ('$HASH', '$MASTER_KEY') ON CONFLICT (api_key_hash) DO NOTHING;" >/dev/null

echo "tenant seeded (label: $LABEL)"
echo "developer API key: $DEV_KEY"
echo "  -> Authorization: Bearer $DEV_KEY"
echo "master key stored for upstream calls (never shown to developers)"
