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
# GitHub OAuth configured at the in-binary mock (ITEM-036 AC4): the smoke signs
# a developer up through the real /auth flow and asserts provisioning. The
# authorize URL must be reachable by the smoke's curl (host port 8091); the
# token/api URLs are called by the proxy inside the compose network. The
# unconfigured surface (login 503) is verified on the prod host instead.
export GITHUB_AUTHORIZE_URL="http://127.0.0.1:8091/login/oauth/authorize"
export GITHUB_TOKEN_URL="http://github-mock:8091/login/oauth/access_token"
export GITHUB_API_URL="http://github-mock:8091"
export GITHUB_CLIENT_ID="smoke-client"
export GITHUB_CLIENT_SECRET="smoke-secret"
export SESSION_SECRET="sk-smoke-session-$(openssl rand -hex 16)"
# Auto-provisioned tenants (ITEM-036) start on this shared upstream key — the
# mock answers guarded replays 200 for any bearer, so the signup key gets a
# real 200 end-to-end.
export UPSTREAM_MASTER_KEY="sk-smoke-master"
# The mock GitHub's one identity (smoke-dev) is also the platform operator
# (ITEM-037): its session must open the operator console, while a plain
# tenant-member session must be 403.
export OPERATOR_GITHUB_LOGINS="smoke-dev"

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
    # The self-serve signup tenant from the ITEM-036 dance (cascade clears its
    # keys + memberships); then the user row itself.
    if [ -n "${SMOKE_USER_ID:-}" ]; then
        psql "DELETE FROM tenants WHERE id IN (SELECT tenant_id FROM tenant_members WHERE user_id = '$SMOKE_USER_ID');" >/dev/null 2>&1 || true
        psql "DELETE FROM users WHERE id = '$SMOKE_USER_ID';" >/dev/null 2>&1 || true
    fi
    if [ -n "${SMOKE_MEMBER_ID:-}" ]; then
        psql "DELETE FROM users WHERE id = '$SMOKE_MEMBER_ID';" >/dev/null 2>&1 || true
    fi
    rm -f "${DANCE_JAR:-}"
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
# is present in the served page). Capture the body first: `curl | grep -q` is
# racy under pipefail — grep -q exits on its first match, closing the pipe and
# making curl die with SIGPIPE (exit 23) whenever the match is late in a
# still-streaming body.
dash=$(curl -fsS "$BASE/v1/dashboard")
printf '%s' "$dash" | grep -qi "no requests yet" \
    || fail "dashboard lacks the empty-state guidance"
echo "ok: empty-state guidance present in the dashboard shell"

printf '%s' "$dash" | grep -q "cost leaks" || fail "dashboard HTML not served"
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

echo "-- ITEM-036 self-serve signup (mock GitHub OAuth, one fixed identity) --"
check_status "/auth/me without a session -> 401" 401 "$BASE/auth/me"

# OAuth dance: /auth/login -> (mock) GitHub authorize -> /auth/callback. Follows
# redirects with a cookie jar so the CSRF-state and session cookies flow.
DANCE_JAR=$(mktemp)
oauth_dance() {
    local location
    location=$(curl -sS -o /dev/null -w '%{redirect_url}' -c "$DANCE_JAR" "$BASE/auth/login")
    [ -n "$location" ] || return 1
    curl -sS -L -b "$DANCE_JAR" -c "$DANCE_JAR" "$location"
}

# First sign-in: success screen showing the raw key exactly once.
BODY1=$(oauth_dance) || fail "first OAuth dance failed"
printf '%s' "$BODY1" | grep -q 'not shown again' || fail "success screen lacks the hash-only note: $BODY1"
SIGNUP_KEY=$(printf '%s' "$BODY1" | grep -o 'sk-inferon-[0-9a-f]\{32\}' | head -1)
[ -n "$SIGNUP_KEY" ] || fail "no key on the success screen: $BODY1"
[ "$(printf '%s' "$BODY1" | grep -o "$SIGNUP_KEY" | wc -l | tr -d ' ')" = "1" ] \
    || fail "the raw key was rendered more than once"
echo "ok: first sign-in shows the key once (self-serve provisioned)"

# Provisioning rows in Postgres: user, owner membership, tenant with the shared
# upstream key, exactly one hashed key.
SMOKE_USER_ID=$(psql "SELECT id FROM users WHERE github_login = 'smoke-dev';")
[ -n "$SMOKE_USER_ID" ] || fail "no user row for the signed-up developer"
MEMBER_COUNT=$(psql "SELECT count(*) FROM tenant_members WHERE user_id = '$SMOKE_USER_ID' AND role = 'owner';")
[ "$MEMBER_COUNT" = "1" ] || fail "no owner membership (rows: $MEMBER_COUNT)"
SIGNUP_TID=$(psql "SELECT tenant_id FROM tenant_members WHERE user_id = '$SMOKE_USER_ID' AND role = 'owner';")
[ -n "$SIGNUP_TID" ] || fail "no provisioned tenant"
[ "$(psql "SELECT upstream_openai_key = 'sk-smoke-master' FROM tenants WHERE id = '$SIGNUP_TID';")" = "t" ] \
    || fail "provisioned tenant lacks the shared upstream key"
[ "$(psql "SELECT count(*) FROM api_keys WHERE tenant_id = '$SIGNUP_TID';")" = "1" ] \
    || fail "expected exactly one first key"
echo "ok: tenant + owner membership + one hashed key in Postgres"

# The session cookie from the dance resolves /auth/me.
ME=$(curl -sS -b "$DANCE_JAR" "$BASE/auth/me")
printf '%s' "$ME" | grep -q '"github_login":"smoke-dev"' || fail "auth/me not signed in after the dance: $ME"
echo "ok: session established (auth/me -> smoke-dev)"

# The revealed key authenticates end-to-end: a guarded request gets the 200
# schema-valid replay (same arms as the seeded guarded tenant).
resp=$(post_and_code "$SIGNUP_KEY" "$GUARDED_BODY")
code=$(printf '%s' "$resp" | tail -1)
body=$(printf '%s' "$resp" | sed '$d')
[ "$code" = "200" ] || fail "signup key: expected guarded 200, got HTTP $code"
printf '%s' "$body" | grep -q 'answer' || fail "signup key request lacked the schema-valid answer: $body"
printf '%s' "$body" | grep -q 'other' && fail "signup key request leaked the cheap lying body: $body"
echo "ok: the revealed key authenticates end-to-end (HTTP 200)"

# Second sign-in (same GitHub identity): lands on the dashboard, no key.
BODY2=$(oauth_dance) || fail "second OAuth dance failed"
printf '%s' "$BODY2" | grep -q 'sk-inferon-' && fail "returning sign-in showed key material"
[ "$(psql "SELECT count(*) FROM api_keys WHERE tenant_id = '$SIGNUP_TID';")" = "1" ] \
    || fail "returning sign-in minted another key"
echo "ok: returning sign-in lands on the dashboard with no second key"

echo "-- ITEM-037 operator console (session-scoped; isolation) --"
# smoke-dev's session (from the dance above) is a platform operator
# (OPERATOR_GITHUB_LOGINS=smoke-dev): the console + operator APIs open.
CONSOLE="$BASE/v1/dashboard/operator"
console=$(curl -sS -b "$DANCE_JAR" "$CONSOLE?days=7")
printf '%s' "$console" | grep -q 'operator console' \
    || fail "operator session did not get the console page: $console"
printf '%s' "$console" | grep -q 'Tenants' \
    || fail "console page lacks the per-tenant list"
code=$(curl -sS -o /dev/null -w '%{http_code}' -b "$DANCE_JAR" "$BASE/v1/dashboard/api/operator/summary?days=7")
[ "$code" = "200" ] || fail "operator session on the operator JSON API: expected 200, got $code"
echo "ok: operator session opens the console + operator APIs"

# The shared operator key still works for machines (console page too).
code=$(curl -sS -o /dev/null -w '%{http_code}' "$CONSOLE?days=7" -H "Authorization: Bearer $OPERATOR_API_KEY")
[ "$code" = "200" ] || fail "operator key on the console: expected 200, got $code"
echo "ok: OPERATOR_API_KEY still opens the console (machine access)"

# Isolation: a real tenant key (DEV_KEY) on the console -> 403.
code=$(curl -sS -o /dev/null -w '%{http_code}' "$CONSOLE?days=7" -H "Authorization: Bearer $DEV_KEY")
[ "$code" = "403" ] || fail "tenant key on the console: expected 403, got $code"
echo "ok: tenant key is 403 on the operator console"

# Isolation at the session seam: a valid tenant-member session (seeded user,
# membership, forged-but-signed cookie) is 403 on the console. The cookie is
# minted with the same SESSION_SECRET the proxy uses, so it verifies.
SMOKE_MEMBER_ID=$(psql "INSERT INTO users (github_id, github_login) VALUES (6660001, 'smoke-member') RETURNING id;")
psql "INSERT INTO tenant_members (tenant_id, user_id, role)
      SELECT id, '$SMOKE_MEMBER_ID', 'member' FROM tenants LIMIT 1;" >/dev/null
MEM_EXP=$(printf '%x' "$(( $(date +%s) + 3600 ))")
MEM_PAYLOAD="$SMOKE_MEMBER_ID.$MEM_EXP"
MEM_MAC=$(printf '%s' "$MEM_PAYLOAD" | openssl dgst -sha256 -hmac "$SESSION_SECRET" -hex | awk '{print $NF}')
MEM_TOKEN="$MEM_PAYLOAD.$MEM_MAC"
code=$(curl -sS -o /dev/null -w '%{http_code}' "$CONSOLE?days=7" -H "Cookie: inferon_session=$MEM_TOKEN")
[ "$code" = "403" ] || fail "tenant-member session on the console: expected 403, got $code"
me=$(curl -sS -H "Cookie: inferon_session=$MEM_TOKEN" "$BASE/auth/me")
printf '%s' "$me" | grep -q '"github_login":"smoke-member"' \
    || fail "member session cookie did not verify (auth/me): $me"
echo "ok: tenant-member session is 403 on the console (session isolation)"

echo "-- ITEM-038 tenant key management (owner vs member, via sessions) --"
# Owner (smoke-dev session from the signup dance) lists + creates + revokes.
code=$(curl -sS -o /dev/null -w '%{http_code}' -b "$DANCE_JAR" "$BASE/v1/dashboard/api/keys")
[ "$code" = "200" ] || fail "owner keys list: expected 200, got $code"
listed=$(curl -sS -b "$DANCE_JAR" "$BASE/v1/dashboard/api/keys")
printf '%s' "$listed" | grep -q '"role":"owner"' || fail "keys list lacks owner role: $listed"
printf '%s' "$listed" | grep -q '"sk-inferon-\|[0-9a-f]\{64\}"' && fail "keys list leaked key/hash material: $listed"
mint=$(curl -sS -b "$DANCE_JAR" -X POST "$BASE/v1/dashboard/api/keys" \
    -H 'Content-Type: application/json' -d '{"label":"smoke-038"}')
DASH_KEY=$(printf '%s' "$mint" | grep -o 'sk-inferon-[0-9a-f]\{32\}' | head -1)
DASH_KEY_ID=$(printf '%s' "$mint" | sed -n 's/.*"key_id":"\([^"]*\)".*/\1/p')
[ -n "$DASH_KEY" ] && [ -n "$DASH_KEY_ID" ] || fail "dashboard mint returned no key: $mint"
echo "ok: owner created a key from the dashboard (shown once)"

# The dashboard-minted key works end-to-end (guarded 200), then revocation
# makes it 401 immediately.
resp=$(post_and_code "$DASH_KEY" "$GUARDED_BODY")
code=$(printf '%s' "$resp" | tail -1)
[ "$code" = "200" ] || fail "dashboard-minted key: expected guarded 200, got HTTP $code"
code=$(curl -sS -o /dev/null -w '%{http_code}' -b "$DANCE_JAR" -X POST \
    "$BASE/v1/dashboard/api/keys/$DASH_KEY_ID/revoke")
[ "$code" = "200" ] || fail "owner revoke: expected 200, got $code"
check_status "chat with revoked dashboard key -> 401" 401 \
    -X POST "$BASE/v1/chat/completions" -H "Content-Type: application/json" \
    -H "Authorization: Bearer $DASH_KEY" \
    -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}'
echo "ok: owner revoked the key; the next request with it is 401"

# Member: can view + create, but revoking is 403.
MEM_EXP=$(printf '%x' "$(( $(date +%s) + 3600 ))")
MEM_PAYLOAD="$SMOKE_MEMBER_ID.$MEM_EXP"
MEM_MAC=$(printf '%s' "$MEM_PAYLOAD" | openssl dgst -sha256 -hmac "$SESSION_SECRET" -hex | awk '{print $NF}')
MEM_TOKEN="$MEM_PAYLOAD.$MEM_MAC"
mint=$(curl -sS -H "Cookie: inferon_session=$MEM_TOKEN" -X POST "$BASE/v1/dashboard/api/keys" \
    -H 'Content-Type: application/json' -d '{"label":"member-key"}')
MEMBER_KEY_ID=$(printf '%s' "$mint" | sed -n 's/.*"key_id":"\([^"]*\)".*/\1/p')
[ -n "$MEMBER_KEY_ID" ] || fail "member mint failed: $mint"
code=$(curl -sS -o /dev/null -w '%{http_code}' -H "Cookie: inferon_session=$MEM_TOKEN" -X POST \
    "$BASE/v1/dashboard/api/keys/$MEMBER_KEY_ID/revoke")
[ "$code" = "403" ] || fail "member revoke: expected 403, got $code"
echo "ok: member can create keys but revoke is 403 (AC2)"

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
