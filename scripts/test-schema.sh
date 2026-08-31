#!/usr/bin/env bash
# ITEM-002 schema seam test. Runs against a real Postgres via docker compose.
# Verifies the auto-applied migration: tenants + traces schema, FK, indexes,
# unique key, and idempotent re-apply. Tears the stack down on exit.
#
# Usage: test-schema.sh   (needs docker; run from the project root)

set -euo pipefail

COMPOSE=(docker compose -f docker-compose.yml)
CONTAINER="inferon-postgres-1"
MIGRATION="db/init/001_init.sql"

cleanup() {
    "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
}
trap cleanup EXIT

psql() {
    docker exec "$CONTAINER" psql -U inferon -d inferon -tAc "$1"
}

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

check_true() {
    local label=$1 query=$2 result
    result=$(psql "$query" | tr -d ' ')
    [ "$result" = "1" ] || [ "$result" = "t" ] || fail "$label (query returned: $result)"
    echo "ok: $label"
}

check_column() {
    local table=$1 column=$2 expected=$3
    local actual
    actual=$(psql "SELECT data_type FROM information_schema.columns WHERE table_name='$table' AND column_name='$column'")
    [ "$actual" = "$expected" ] || fail "column $table.$column (expected $expected, got $actual)"
    echo "ok: column $table.$column = $expected"
}

check_table() {
    check_true "table $1 exists" "SELECT to_regclass('public.$1') IS NOT NULL"
}

wait_healthy() {
    "${COMPOSE[@]}" up -d postgres
    for _ in $(seq 1 30); do
        status=$(docker inspect -f '{{.State.Health.Status}}' "$CONTAINER" 2>/dev/null || echo none)
        [ "$status" = "healthy" ] && return 0
        sleep 2
    done
    echo "--- docker ps ---"
    docker ps -a | head -5
    echo "--- postgres logs ---"
    docker logs "$CONTAINER" 2>&1 | tail -25
    fail "postgres did not become healthy (last status: $status)"
}

echo "starting postgres (auto-applies $MIGRATION on first init)..."
wait_healthy

echo "--- initdb.d contents ---"
docker exec "$CONTAINER" ls -la /docker-entrypoint-initdb.d/
echo "--- tables after init ---"
docker exec "$CONTAINER" psql -U inferon -d inferon -c '\dt'

check_table tenants
check_table traces

check_column tenants id uuid
check_column tenants api_key_hash "character varying"
check_column tenants upstream_openai_key text
check_column tenants upstream_anthropic_key text
check_column tenants created_at "timestamp with time zone"

check_column traces id uuid
check_column traces tenant_id uuid
check_column traces request_model "character varying"
check_column traces system_prompt_hash "character varying"
check_column traces prompt_tokens integer
check_column traces completion_tokens integer
check_column traces time_to_first_token_ms integer
check_column traces total_duration_ms integer
check_column traces upstream_cost_usd numeric
check_column traces waste_detected jsonb
check_column traces status_code integer
check_column traces raw_prompt jsonb
check_column traces raw_completion jsonb
check_column traces created_at "timestamp with time zone"

check_true "tenants.api_key_hash is unique" \
    "SELECT count(*) FROM information_schema.table_constraints WHERE table_name='tenants' AND constraint_type='UNIQUE'"

check_true "traces.tenant_id FK cascades" \
    "SELECT count(*) FROM information_schema.table_constraints tc JOIN information_schema.referential_constraints rc ON tc.constraint_name=rc.constraint_name WHERE tc.table_name='traces' AND tc.constraint_type='FOREIGN KEY' AND rc.delete_rule='CASCADE'"

check_true "index idx_traces_tenant_created exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='traces' AND indexname='idx_traces_tenant_created'"

check_true "index idx_traces_prompt_hash exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='traces' AND indexname='idx_traces_prompt_hash'"

echo "re-applying migration (idempotency)..."
docker exec -i "$CONTAINER" psql -U inferon -d inferon -q < "$MIGRATION" || fail "idempotent re-apply errored"
echo "ok: idempotent re-apply"

echo "ALL SCHEMA CHECKS PASSED"
