#!/usr/bin/env bash
# Self-hosted install smoke (ITEM-083 AC4). Proves the documented path, not a
# bespoke one:
#   docker compose --profile self-hosted up -d --build
# starts proxy + web + postgres + redis (no Caddy), the documented migration
# step re-applies cleanly, and a seeded tenant key proxies a request end to end.
#
# CI/local only. It uses its own compose project name, so it can never touch a
# real stack — do NOT run it on the durable host (it would build next to prod).
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${SMOKE_PROJECT:-inferon-selfhosted-smoke}"
PORT="${SMOKE_PORT:-18080}"
WEB_PORT="${SMOKE_WEB_PORT:-13000}"
export PORT WEB_PORT
export WEB_BIND="127.0.0.1"
# Both on the command line: an explicit --profile overrides COMPOSE_PROFILES,
# and the lying mock is the only "model" this smoke may call (no egress).
PROFILES=(--profile self-hosted --profile mock)
# The lying mock upstream is the "model": it streams a plain SSE 200 for any
# unguarded request, so the smoke needs no external provider.
export UPSTREAM_URL="http://mock-upstream:8090/v1/chat/completions"
export UPSTREAM_MASTER_KEY="sk-selfhosted-master"
export POSTGRES_CONTAINER="$PROJECT-postgres-1"
export RUST_LOG=warn
COMPOSE=(docker compose -p "$PROJECT" -f docker-compose.yml)

cleanup() {
    "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

# 1. The documented compose file must parse with the documented profile.
"${COMPOSE[@]}" "${PROFILES[@]}" config --quiet
echo "ok: compose config validates (self-hosted profile)"

# 2. Start it exactly as the guide says.
"${COMPOSE[@]}" "${PROFILES[@]}" up -d --build

# 3. Both human-visible surfaces answer.
for i in $(seq 1 120); do
    if curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 \
        && curl -fsS "http://127.0.0.1:$WEB_PORT/api/health" >/dev/null 2>&1; then
        echo "ok: proxy + web healthy"
        break
    fi
    if [ "$i" -eq 120 ]; then
        echo "stack did not become healthy" >&2
        "${COMPOSE[@]}" logs --tail 60 >&2
        exit 1
    fi
    sleep 2
done

# 4. The documented upgrade step is idempotent.
bash ./scripts/migrate.sh >/dev/null
bash ./scripts/migrate.sh >/dev/null
echo "ok: migrations re-apply cleanly"

# 5. Onboard a tenant the documented way and proxy one request.
KEY=$(bash ./scripts/seed-tenant.sh "$UPSTREAM_MASTER_KEY" selfhosted-smoke \
    | awk '/developer API key:/ {print $4}')
[ -n "$KEY" ] || { echo "seed-tenant.sh did not print a key" >&2; exit 1; }

STATUS=$(curl -sS -o /tmp/selfhosted-smoke.json -w '%{http_code}' \
    "http://127.0.0.1:$PORT/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" \
    -H 'content-type: application/json' \
    -d '{"model":"mock-model","stream":true,"messages":[{"role":"user","content":"hello"}]}')
[ "$STATUS" = "200" ] || {
    echo "proxied request failed (HTTP $STATUS)" >&2
    cat /tmp/selfhosted-smoke.json >&2
    exit 1
}
grep -q 'data:' /tmp/selfhosted-smoke.json
echo "ok: proxied a request end to end"
echo "self-hosted smoke passed"
