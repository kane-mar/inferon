#!/usr/bin/env bash
# Deployment seam smoke for the full proxy surface (ITEM-012). Runs on a host
# with Docker. Brings the stack up — including the lying mock upstream under
# the `mock` compose profile (ITEM-027) — seeds throwaway tenants, then asserts:
#   healthz 200, chat 401 without a key, chat 502 with a valid key (mock
#   answers 502 to unguarded traffic: auth boundary without external calls),
#   a lying cheap upstream forces a transparent frontier fallback and the
#   client still gets a valid 200 (ITEM-027 AC3), the per-tenant guard
#   opt-out restores raw passthrough (ITEM-027 AC1),
#   dashboard/operator views 200 with the right keys and 401/403 without.
# Cleans up the throwaway tenants on exit.

set -euo pipefail
cd "$(dirname "$0")/.."

COMPOSE=(docker compose -f docker-compose.yml)
PORT="${PORT:-8080}"
BASE="http://127.0.0.1:$PORT"
CONTAINER="inferon-postgres-1"
# The lying mock upstream (ITEM-027 AC3): schema-violating SSE for the cheap
# tier, schema-valid SSE for the guard's frontier replay, 502 for everything
# else. Profile-gated so prod deploys never run it.
export COMPOSE_PROFILES="mock"
export UPSTREAM_URL="http://mock-upstream:8090/v1/chat/completions"
# Operator credential (ITEM-013/020): enables the platform-wide operator views.
# Generated here so the smoke run exercises the operator surface end-to-end.
export OPERATOR_API_KEY="sk-smoke-op-$(openssl rand -hex 8)"

psql() {
    # -q suppresses the command tag so INSERT ... RETURNING yields only the row.
    docker exec "$CONTAINER" psql -U inferon -d inferon -qtAc "$1"
}

cleanup() {
    # api_keys rows cascade on tenant delete; match tenants through their key
    # rows (ITEM-034: key hashes live on api_keys, not tenants).
    if [ -n "${DEV_KEY_HASH:-}" ]; then
        psql "DELETE FROM tenants t USING api_keys k WHERE k.tenant_id = t.id AND k.key_hash = '$DEV_KEY_HASH';" >/dev/null 2>&1 || true
    fi
    if [ -n "${OPT_OUT_HASH:-}" ]; then
        psql "DELETE FROM tenants t USING api_keys k WHERE k.tenant_id = t.id AND k.key_hash = '$OPT_OUT_HASH';" >/dev/null 2>&1 || true
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

# Seed a guarded tenant plus an opt-out tenant (ITEM-027 AC1): sha256 hashes
# + a placeholder master key; the opt-out tenant's configuration row disables
# the schema guard. ITEM-034: key hashes live on api_keys (one row per key).
DEV_KEY="sk-smoke-$(openssl rand -hex 8)"
DEV_KEY_HASH=$(printf '%s' "$DEV_KEY" | shasum -a 256 | awk '{print $1}')
GUARDED_ID=$(psql "INSERT INTO tenants (upstream_openai_key) VALUES ('sk-smoke-master') RETURNING id;")
psql "INSERT INTO api_keys (tenant_id, label, key_hash)
      VALUES ('$GUARDED_ID', 'smoke', '$DEV_KEY_HASH') ON CONFLICT (key_hash) DO NOTHING;" >/dev/null
OPT_OUT_KEY="sk-smoke-opt-$(openssl rand -hex 8)"
OPT_OUT_HASH=$(printf '%s' "$OPT_OUT_KEY" | shasum -a 256 | awk '{print $1}')
OPT_OUT_ID=$(psql "INSERT INTO tenants (upstream_openai_key) VALUES ('sk-smoke-master') RETURNING id;")
psql "INSERT INTO api_keys (tenant_id, label, key_hash)
      VALUES ('$OPT_OUT_ID', 'smoke-opt', '$OPT_OUT_HASH') ON CONFLICT (key_hash) DO NOTHING;" >/dev/null
psql "INSERT INTO tenant_configurations (tenant_id, guard_enabled)
      VALUES ('$OPT_OUT_ID', false);" >/dev/null
echo "ok: seeded guarded + opt-out tenants (api_keys rows)"

echo "-- auth boundary (no external calls: the mock answers 502 to unguarded traffic) --"
check_status "chat without key -> 401" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
check_status "chat with wrong key -> 401" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer sk-wrong" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
check_status "chat with tenant key -> 502 (auth passed, upstream refused)" 502 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $DEV_KEY" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'

echo "-- schema guard: lying cheap upstream forces a frontier fallback (ITEM-027 AC3) --"
# A guarded request (cheap model + json_schema contract + stream): the mock
# answers a schema-violating SSE as gpt-4o-mini, so the proxy must discard it
# and replay on the frontier (deepseek-v4-pro), which answers schema-valid.
GUARDED_BODY='{"model":"gpt-4o-mini","stream":true,"response_format":{"type":"json_schema","json_schema":{"name":"answer","schema":{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]}}},"messages":[{"role":"user","content":"smoke-guarded"}]}'
post_and_code() {
    local key=$1 body=$2
    curl -sS -w '\n%{http_code}' -X POST "$BASE/v1/chat/completions" \
        -H "Content-Type: application/json" -H "Authorization: Bearer $key" -d "$body"
}
resp=$(post_and_code "$DEV_KEY" "$GUARDED_BODY")
code=$(printf '%s' "$resp" | tail -1)
body=$(printf '%s' "$resp" | sed '$d')
[ "$code" = "200" ] || fail "guarded request: expected fallback 200, got HTTP $code"
# SSE double-encodes the content JSON (quotes escaped), so match bare words.
printf '%s' "$body" | grep -q 'answer' || fail "fallback body lacks the schema-valid answer: $body"
printf '%s' "$body" | grep -q 'other' && fail "cheap lying body leaked to the client: $body"
echo "ok: guarded request answered with the frontier's schema-valid replay (HTTP $code)"

# The opt-out tenant sees the raw (lying) cheap body — guard bypassed.
resp=$(post_and_code "$OPT_OUT_KEY" "$GUARDED_BODY")
code=$(printf '%s' "$resp" | tail -1)
body=$(printf '%s' "$resp" | sed '$d')
[ "$code" = "200" ] || fail "opt-out request: expected raw 200, got HTTP $code"
printf '%s' "$body" | grep -q 'other' || fail "opt-out tenant should see the cheap body untouched: $body"
echo "ok: opt-out tenant gets the raw cheap body, no fallback (ITEM-027 AC1)"

# Traces settle in the DB after a telemetry flush; assert the guard events.
sleep 2
fb=$(psql "SELECT count(*) FROM traces t JOIN api_keys k ON k.tenant_id = t.tenant_id WHERE k.key_hash = '$DEV_KEY_HASH' AND t.guard_event->>'outcome' = 'fallback';")
[ "$fb" -ge 1 ] || fail "no fallback guard event recorded for the guarded tenant (rows: $fb)"
echo "ok: fallback guard event persisted (outcome=fallback)"
noev=$(psql "SELECT count(*) FROM traces t JOIN api_keys k ON k.tenant_id = t.tenant_id WHERE k.key_hash = '$OPT_OUT_HASH' AND t.guard_event IS NOT NULL;")
[ "$noev" = "0" ] || fail "opt-out tenant must record no guard event (rows: $noev)"
echo "ok: opt-out tenant traces carry no guard event (additive telemetry)"

echo "-- dashboard surface --"
check_status "summary without key -> 401" 401 "$BASE/v1/dashboard/api/summary"
check_status "summary with key -> 200" 200 \
    "$BASE/v1/dashboard/api/summary?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "by-model with key -> 200" 200 \
    "$BASE/v1/dashboard/api/by-model?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "waste with key -> 200" 200 \
    "$BASE/v1/dashboard/api/waste?days=7" -H "Authorization: Bearer $DEV_KEY"

# ITEM-019 AC4: the window selector works for 7/30/90 on every view.
for d in 7 30 90; do
    for view in summary by-model by-prompt waste; do
        check_status "$view?days=$d -> 200" 200 \
            "$BASE/v1/dashboard/api/$view?days=$d" -H "Authorization: Bearer $DEV_KEY"
    done
done

# ITEM-019 AC3: currency comes back as a decimal string, never scientific
# notation (1.6e-5) and never a bare float.
cost=$(curl -sS "$BASE/v1/dashboard/api/summary?days=7" -H "Authorization: Bearer $DEV_KEY" \
    | grep -o '"total_cost_usd":"[^"]*"' | cut -d'"' -f4)
printf '%s' "$cost" | grep -Eq '^[0-9]+\.[0-9]+$' \
    || fail "cost is not a decimal string (got: $cost)"
printf '%s' "$cost" | grep -Eq 'e[-+]' \
    && fail "cost rendered in scientific notation: $cost"
echo "ok: currency renders as a plain decimal string (got \"$cost\")"

# ITEM-019 AC3: totals are consistent across views — the summary total equals
# the sum of the by-model rows (same trace-level math, ITEM-008).
sum_models=$(curl -sS "$BASE/v1/dashboard/api/by-model?days=7" -H "Authorization: Bearer $DEV_KEY" \
    | grep -o '"cost_usd":"[^"]*"' | cut -d'"' -f4 | awk '{ s += $1 } END { printf "%.6f", s }')
[ "$sum_models" = "$cost" ] \
    || fail "summary total ($cost) != by-model sum ($sum_models)"
echo "ok: summary total equals the by-model sum ($cost)"

# ITEM-019 AC2: an empty tenant sees a meaningful empty state (the shell text
# is present in the served page).
curl -fsS "$BASE/v1/dashboard" | grep -qi "no requests yet" \
    || fail "dashboard lacks the empty-state guidance"
echo "ok: empty-state guidance present in the dashboard shell"

curl -fsS "$BASE/v1/dashboard" | grep -q "cost leaks" || fail "dashboard HTML not served"
echo "ok: dashboard HTML served"

echo "-- operator surface (ITEM-020 alerts view) --"
check_status "operator alerts without key -> 401" 401 \
    "$BASE/v1/dashboard/api/operator/alerts?days=7"
check_status "operator alerts with tenant key -> 403" 403 \
    "$BASE/v1/dashboard/api/operator/alerts?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "operator alerts with operator key -> 200" 200 \
    "$BASE/v1/dashboard/api/operator/alerts?days=7" -H "Authorization: Bearer $OPERATOR_API_KEY"

# ITEM-026: guard telemetry views (tenant + operator) follow the same gates.
check_status "guard without key -> 401" 401 \
    "$BASE/v1/dashboard/api/guard?days=7"
check_status "guard with tenant key -> 200" 200 \
    "$BASE/v1/dashboard/api/guard?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "operator guard without key -> 401" 401 \
    "$BASE/v1/dashboard/api/operator/guard?days=7"
check_status "operator guard with tenant key -> 403" 403 \
    "$BASE/v1/dashboard/api/operator/guard?days=7" -H "Authorization: Bearer $DEV_KEY"
check_status "operator guard with operator key -> 200" 200 \
    "$BASE/v1/dashboard/api/operator/guard?days=7" -H "Authorization: Bearer $OPERATOR_API_KEY"
for d in 7 30 90; do
    check_status "guard?days=$d -> 200" 200 \
        "$BASE/v1/dashboard/api/guard?days=$d" -H "Authorization: Bearer $DEV_KEY"
done

echo "-- ITEM-034 operator key API (mint/list/revoke; revoked key -> 401) --"
KEYS_URL="$BASE/v1/dashboard/api/operator/keys"
check_status "keys list without key -> 401" 401 "$KEYS_URL?tenant_id=$GUARDED_ID"
check_status "keys list with tenant key -> 403" 403 \
    "$KEYS_URL?tenant_id=$GUARDED_ID" -H "Authorization: Bearer $DEV_KEY"
check_status "keys list with operator key -> 200" 200 \
    "$KEYS_URL?tenant_id=$GUARDED_ID" -H "Authorization: Bearer $OPERATOR_API_KEY"

# Mint a second key for the guarded tenant via the operator API (AC3).
mint=$(curl -sS -X POST "$KEYS_URL" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $OPERATOR_API_KEY" \
    -d "{\"tenant_id\":\"$GUARDED_ID\",\"label\":\"smoke-minted\"}")
MINTED_KEY=$(printf '%s' "$mint" | sed -n 's/.*"key":"\([^"]*\)".*/\1/p')
MINTED_KEY_ID=$(printf '%s' "$mint" | sed -n 's/.*"key_id":"\([^"]*\)".*/\1/p')
[ -n "$MINTED_KEY" ] && [ -n "$MINTED_KEY_ID" ] || fail "mint returned no key/key_id: $mint"
printf '%s' "$MINTED_KEY" | grep -q '^sk-inferon-' || fail "minted key has the wrong prefix: $MINTED_KEY"
echo "ok: minted a second tenant key via the operator API (label smoke-minted)"

# The minted key authenticates (mock answers 502 to unguarded traffic: auth
# boundary crossed, upstream refused — same as the seeded key).
check_status "chat with minted key -> 502 (auth passed)" 502 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $MINTED_KEY" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'

# List shows metadata only — never key material (AC3).
listed=$(curl -sS "$KEYS_URL?tenant_id=$GUARDED_ID" -H "Authorization: Bearer $OPERATOR_API_KEY")
printf '%s' "$listed" | grep -q 'smoke-minted' || fail "list lacks the minted key label: $listed"
printf '%s' "$listed" | grep -q "$MINTED_KEY" && fail "list leaked key material: $listed"
echo "ok: key list shows lifecycle metadata, no key material"

# Revoke via the operator API; the same raw key now returns 401 (AC2).
code=$(curl -sS -o /dev/null -w "%{http_code}" -X POST \
    "$BASE/v1/dashboard/api/operator/keys/$MINTED_KEY_ID/revoke" \
    -H "Authorization: Bearer $OPERATOR_API_KEY")
[ "$code" = "200" ] || fail "revoke expected 200, got $code"
check_status "chat with revoked key -> 401 (AC2)" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $MINTED_KEY" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
echo "ok: revoked key stopped authenticating immediately"

echo "ALL SMOKE CHECKS PASSED"
