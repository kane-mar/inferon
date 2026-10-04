#!/usr/bin/env bash
# Apply db/init/*.sql in order to the running Postgres.
#
# The Postgres docker-entrypoint only auto-applies db/init on a *fresh* volume;
# every later migration has to be applied explicitly. Every file in db/init is
# written to be idempotent (CREATE/ALTER ... IF NOT EXISTS, guarded backfills),
# so the safe deploy-time action is: re-apply them ALL, in order. That makes
# deploys and CI/CD migration-aware without a migration-tracking table.
#
# Usage: migrate.sh   (from the project root on the host, stack's PG running)

set -euo pipefail
cd "$(dirname "$0")/.."

CONTAINER="${POSTGRES_CONTAINER:-inferon-postgres-1}"

# Wait for Postgres to accept connections (fresh volumes run initdb first).
for _ in $(seq 1 60); do
    if docker exec "$CONTAINER" pg_isready -U inferon -d inferon >/dev/null 2>&1; then
        break
    fi
    sleep 2
done

applied=0
for migration in db/init/*.sql; do
    docker exec -i "$CONTAINER" psql -U inferon -d inferon -q -v ON_ERROR_STOP=1 < "$migration"
    applied=$((applied + 1))
done
echo "migrations applied (idempotent re-apply): $applied"
