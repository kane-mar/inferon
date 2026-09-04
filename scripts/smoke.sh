#!/usr/bin/env bash
# Deployment seam smoke for the full proxy surface (ITEM-012). Runs on a host
# with Docker. Brings the stack up, seeds a throwaway tenant, then asserts:
#   healthz 200, chat 401 without a key, chat 502 with a valid key against an
#   unreachable upstream (proves the auth boundary without external calls),
#   dashboard summary/by-model/waste 200 with the key and 401 without.
# Cleans up the throwaway tenant on exit.

set -euo pipefail
cd "$(dirname "$0")/.."

COMPOSE=(docker compose -f docker-compose.yml)
PORT="${PORT:-8080}"
BASE="http://127.0.0.1:$PORT"
CONTAINER="inferon-postgres-1"
# Unreachable upstream: authenticated requests must pass auth then fail fast.
export UPSTREAM_URL="http://127.0.0.1:9/v1/chat/completions"

psql() {
    docker exec "$CONTAINER" psql -U inferon -d inferon -tAc "$1"
}

cleanup() {
    if [ -n "${DEV_KEY_HASH:-}" ]; then
        psql "DELETE FROM tenants WHERE api_key_hash = '$DEV_KEY_HASH';" >/dev/null 2>&1 || true
    fi
    "${COMPOSE[@]}" down >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() { echo "FAIL: $1" >&2; exit 1; }
check_status() {
    local label=$1 want=$2 got
    got=$(curl -sS -o /dev/null -w "%{http_code}" "${@:3}" || true)
    [ "$got" = "$want" ] || fail "$label (expected HTTP $want, got $got)"
    echo "ok: $label (HTTP $got)"
}

docker compose config --quiet
docker compose up -d --build
trap cleanup EXIT

echo "-- waiting for proxy health --"
for i in $(seq 1 60); do
    if curl -fsS "$BASE/healthz" >/dev/null 2>&1; then
        echo "ok: proxy healthy after ${i}s"
        break
    fi
    sleep 1
    [ "$i" = 60 ] && fail "proxy did not become healthy within 60s"
done

# Seed a throwaway tenant: sha256(dev key) + a placeholder master key.
DEV_KEY="sk-smoke-$(openssl rand -hex 8)"
DEV_KEY_HASH=$(printf '%s' "$DEV_KEY" | shasum -a 256 | awk '{print $1}')
psql "INSERT INTO tenants (api_key_hash, upstream_openai_key)
      VALUES ('$DEV_KEY_HASH', 'sk-smoke-master') ON CONFLICT DO NOTHING;" >/dev/null
echo "ok: seeded throwaway tenant"

echo "-- auth boundary (no external calls: upstream is unroutable) --"
check_status "chat without key -> 401" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
check_status "chat with wrong key -> 401" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer sk-wrong" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
check_status "chat with tenant key -> 502 (auth passed, upstream dead)" 502 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $DEV_KEY" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'

echo "-- dashboard surface --"
check_status "summary without key -> 401" 401 "$BASE/v1/dashboard/api/summary"
check_status "summary with key -> 200" 200 \
    "$BASE/v1/dashboard/api/summary?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "by-model with key -> 200" 200 \
    "$BASE/v1/dashboard/api/by-model?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "waste with key -> 200" 200 \
    "$BASE/v1/dashboard/api/waste?days=7" -H "Authorization: Bearer $DEV_KEY"

curl -fsS "$BASE/v1/dashboard" | grep -q "cost leaks" || fail "dashboard HTML not served"
echo "ok: dashboard HTML served"

echo "ALL SMOKE CHECKS PASSED"
