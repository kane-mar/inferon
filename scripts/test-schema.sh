#!/usr/bin/env bash
# ITEM-002 schema seam test. Runs against a real Postgres via docker compose.
# Verifies the auto-applied migration: tenants + traces schema, FK, indexes,
# unique key, and idempotent re-apply. Tears the stack down on exit.
#
# Usage: test-schema.sh   (needs docker; run from the project root)

set -euo pipefail

COMPOSE=(docker compose -f docker-compose.yml)
CONTAINER="inferon-postgres-1"
MIGRATIONS="db/init/001_init.sql db/init/002_extensions.sql db/init/003_alerts.sql db/init/004_guard.sql db/init/005_guard_optout.sql db/init/006_api_keys.sql db/init/007_users.sql db/init/008_tenant_members.sql db/init/009_users_operator.sql"

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

echo "starting postgres (auto-applies db/init/*.sql on first init)..."
wait_healthy

echo "--- initdb.d contents ---"
docker exec "$CONTAINER" ls -la /docker-entrypoint-initdb.d/
echo "--- tables after init ---"
docker exec "$CONTAINER" psql -U inferon -d inferon -c '\dt'

check_table tenants
check_table traces

check_column tenants id uuid
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
check_column traces truncated boolean
check_column traces guard_event jsonb
check_true "partial index idx_traces_guard_reason exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='traces' AND indexname='idx_traces_guard_reason'"

check_table system_prompt_counts
check_table tenant_configurations
check_column system_prompt_counts hash "character varying"
check_column system_prompt_counts count integer
check_column tenant_configurations tenant_id uuid
check_column tenant_configurations budget_limit_cents bigint
check_column tenant_configurations current_spend_cents bigint
check_column tenant_configurations cheap_fallback_model text

check_true "traces.tenant_id FK cascades" \
    "SELECT count(*) FROM information_schema.table_constraints tc JOIN information_schema.referential_constraints rc ON tc.constraint_name=rc.constraint_name WHERE tc.table_name='traces' AND tc.constraint_type='FOREIGN KEY' AND rc.delete_rule='CASCADE'"

check_true "index idx_traces_tenant_created exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='traces' AND indexname='idx_traces_tenant_created'"

check_true "index idx_traces_prompt_hash exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='traces' AND indexname='idx_traces_prompt_hash'"

# ITEM-020 migration 003: alerts table + per-tenant alert thresholds.
check_table alerts
check_column alerts tenant_id uuid
check_column alerts class "character varying"
check_column alerts status "character varying"
check_column alerts detail jsonb
check_column alerts threshold jsonb
check_column alerts first_fired_at "timestamp with time zone"
check_column alerts last_fired_at "timestamp with time zone"
check_column alerts cleared_at "timestamp with time zone"
check_column tenant_configurations alert_spend_multiplier numeric
check_column tenant_configurations alert_spend_floor_cents bigint
check_column tenant_configurations alert_waste_daily_threshold integer
# ITEM-027 migration 005: per-tenant guard opt-out, default ON.
check_column tenant_configurations guard_enabled boolean
check_true "guard_enabled defaults to true" \
    "SELECT column_default = 'true' FROM information_schema.columns WHERE table_name='tenant_configurations' AND column_name='guard_enabled'"

# ITEM-034 migration 006: api_keys rows (multi-key + revoke). On a fresh
# volume the retired tenants.api_key_hash column is already gone — auth
# resolves through api_keys.
check_table api_keys
check_column api_keys id uuid
check_column api_keys tenant_id uuid
check_column api_keys label text
check_column api_keys key_hash "character varying"
check_column api_keys created_at "timestamp with time zone"
check_column api_keys revoked_at "timestamp with time zone"
check_column api_keys last_used_at "timestamp with time zone"
check_true "api_keys.key_hash unique index exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='api_keys' AND indexname='idx_api_keys_hash'"
check_true "api_keys.tenant_id FK cascades" \
    "SELECT count(*) FROM information_schema.table_constraints tc JOIN information_schema.referential_constraints rc ON tc.constraint_name=rc.constraint_name WHERE tc.table_name='api_keys' AND tc.constraint_type='FOREIGN KEY' AND rc.delete_rule='CASCADE'"
check_true "tenants.api_key_hash retired on a fresh volume" \
    "SELECT count(*) = 0 FROM information_schema.columns WHERE table_name='tenants' AND column_name='api_key_hash'"

# ITEM-034 AC1 upgrade path: a pre-006 database (api_key_hash column present)
# backfills each tenant's single key as its first api_keys row, then retires
# the column. Simulated here by re-adding the column + a legacy tenant, then
# re-applying 006.
psql "ALTER TABLE tenants ADD COLUMN api_key_hash VARCHAR(64) UNIQUE;" >/dev/null 2>&1
psql "INSERT INTO tenants (api_key_hash, upstream_openai_key)
      VALUES ('legacyhash0001', 'sk-legacy-master');" >/dev/null 2>&1
docker exec -i "$CONTAINER" psql -U inferon -d inferon -q < db/init/006_api_keys.sql
check_true "upgrade backfills the legacy key as an api_keys row" \
    "SELECT count(*) FROM api_keys WHERE key_hash='legacyhash0001' AND label='migrated'"
check_true "upgrade retires api_key_hash again" \
    "SELECT count(*) = 0 FROM information_schema.columns WHERE table_name='tenants' AND column_name='api_key_hash'"

echo "re-applying migrations (idempotency)..."
for m in $MIGRATIONS; do
    docker exec -i "$CONTAINER" psql -U inferon -d inferon -q < "$m" || fail "idempotent re-apply errored on $m"
done
echo "ok: idempotent re-apply (001..009)"
# A re-applied 006 must not duplicate the earlier backfill: with the column
# gone its backfill branch is skipped.
check_true "re-applied 006 did not duplicate the backfilled key" \
    "SELECT count(*) FROM api_keys WHERE key_hash='legacyhash0001'"

# ITEM-035 migration 007: users (human accounts keyed by GitHub id).
check_table users
check_column users id uuid
check_column users github_id bigint
check_column users github_login text
check_column users created_at "timestamp with time zone"
check_column users last_login_at "timestamp with time zone"
check_true "users.github_id unique index exists" \
    "SELECT count(*) FROM pg_indexes WHERE tablename='users' AND indexname='idx_users_github_id'"

# ITEM-036 migration 008: tenant_members (owner/member links).
check_table tenant_members
check_column tenant_members tenant_id uuid
check_column tenant_members user_id uuid
check_column tenant_members role text
check_column tenant_members created_at "timestamp with time zone"
check_true "tenant_members PK covers (tenant, user)" \
    "SELECT count(*) FROM information_schema.table_constraints WHERE table_name='tenant_members' AND constraint_type='PRIMARY KEY'"
check_true "tenant_members.tenant_id FK cascades" \
    "SELECT count(*) FROM information_schema.table_constraints tc JOIN information_schema.referential_constraints rc ON tc.constraint_name=rc.constraint_name WHERE tc.table_name='tenant_members' AND tc.constraint_type='FOREIGN KEY' AND rc.delete_rule='CASCADE'"
check_true "tenant_members.user_id FK cascades" \
    "SELECT count(*) FROM information_schema.table_constraints tc JOIN information_schema.referential_constraints rc ON tc.constraint_name=rc.constraint_name WHERE tc.table_name='tenant_members' AND tc.constraint_type='FOREIGN KEY' AND rc.delete_rule='CASCADE'"
check_true "tenant_members.role restricted to owner|member" \
    "SELECT count(*) FROM pg_constraint WHERE conrelid = 'tenant_members'::regclass AND contype = 'c' AND pg_get_constraintdef(oid) LIKE '%owner%'"

# ITEM-037 migration 009: users.is_operator (platform operator flag).
check_column users is_operator boolean
check_true "users.is_operator defaults to false" \
    "SELECT column_default = 'false' FROM information_schema.columns WHERE table_name='users' AND column_name='is_operator'"

echo "ALL SCHEMA CHECKS PASSED"
