#!/usr/bin/env bash
# ITEM-092 — verify the sub-millisecond proxy-overhead claim on a deployed host.
#
# Sends warmed requests through the real proxy and reads `traces.proxy_us`
# (request start -> before the first upstream dispatch, so the upstream's own
# time is excluded). The tenant is throwaway and its master key is bogus, so the
# upstream answers 401: the probe costs nothing and never touches a real model.
#
# Fails when p50 >= 1000us or p95 >= 2000us. Runs from the deploy job on every
# deployment (scripts/migrate.sh has already added `proxy_us`).
#
# Usage (on the host):   bash ./scripts/perf-check.sh
# Environment:
#   PERF_BASE        proxy base URL            (default http://127.0.0.1:${PORT:-8080})
#   PERF_REQUESTS    measured requests         (default 40)
#   PERF_P50_US      p50 budget in µs          (default 1000)
#   PERF_P95_US      p95 budget in µs          (default 2000)
#   POSTGRES_CONTAINER                         (default inferon-postgres-1)

set -euo pipefail
cd "$(dirname "$0")/.."

BASE="${PERF_BASE:-http://127.0.0.1:${PORT:-8080}}"
CONTAINER="${POSTGRES_CONTAINER:-inferon-postgres-1}"
REQUESTS="${PERF_REQUESTS:-40}"
P50_BUDGET="${PERF_P50_US:-1000}"
P95_BUDGET="${PERF_P95_US:-2000}"

psql() {
    docker exec "$CONTAINER" psql -U inferon -d inferon -qtAc "$1"
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | awk '{print $1}'
    else
        shasum -a 256 | awk '{print $1}'
    fi
}

if ! psql "SELECT 1" >/dev/null 2>&1; then
    echo "perf-check: postgres is not reachable (is the stack up?)" >&2
    exit 1
fi
if ! psql "SELECT column_name FROM information_schema.columns
           WHERE table_name='traces' AND column_name='proxy_us'" | grep -q proxy_us; then
    echo "perf-check: traces.proxy_us is missing — run scripts/migrate.sh first" >&2
    exit 1
fi

TENANT=$(psql "INSERT INTO tenants (upstream_openai_key) VALUES ('sk-perf-check-bogus') RETURNING id;")
KEY="sk-inferon-perf-$(openssl rand -hex 12)"
HASH=$(printf '%s' "$KEY" | sha256)
psql "INSERT INTO api_keys (tenant_id, label, key_hash)
      VALUES ('$TENANT', 'perf-check', '$HASH');" >/dev/null

cleanup() {
    psql "DELETE FROM tenants WHERE id = '$TENANT';" >/dev/null 2>&1 || true
}
trap cleanup EXIT

BODY='{"model":"perf-check","messages":[{"role":"user","content":"ping"}]}'

# Warm-up: auth lookup + config snapshot + route + spend, then discard the sample
# so we measure the steady state the claim is about.
curl -sS -o /dev/null --max-time 10 "$BASE/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" -H 'content-type: application/json' -d "$BODY" || true
sleep 0.3
# Keep the burst inside the 5 s config TTL so every sample is a warm request.
seq 1 "$REQUESTS" | xargs -P 4 -I{} curl -sS -o /dev/null --max-time 10 \
    "$BASE/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" -H 'content-type: application/json' -d "$BODY" || true
# Telemetry flushes every 500 ms.
sleep 2

read -r COUNT P50 P95 <<<"$(psql "SELECT count(*),
        COALESCE(round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us)), 0),
        COALESCE(round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us)), 0)
    FROM traces WHERE tenant_id = '$TENANT' AND proxy_us IS NOT NULL;")"

echo "proxy overhead: ${COUNT} requests · p50 ${P50}us · p95 ${P95}us"
echo "budget:          p50 < ${P50_BUDGET}us · p95 < ${P95_BUDGET}us"

if [ "${COUNT:-0}" -lt "$REQUESTS" ]; then
    echo "perf-check: only $COUNT measured requests (expected $REQUESTS) — telemetry or the proxy is dropping traces" >&2
    exit 1
fi
if [ "$P50" -ge "$P50_BUDGET" ]; then
    echo "perf-check FAILED: p50 ${P50}us >= ${P50_BUDGET}us" >&2
    exit 1
fi
if [ "$P95" -ge "$P95_BUDGET" ]; then
    echo "perf-check FAILED: p95 ${P95}us >= ${P95_BUDGET}us" >&2
    exit 1
fi
echo "ok: sub-millisecond proxy overhead"
