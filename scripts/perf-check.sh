#!/usr/bin/env bash
# ITEM-092 — verify the sub-millisecond proxy-overhead claim on a deployed host.
#
# Sends warmed requests through the real proxy and reads `traces.proxy_us`
# (request start -> before the first upstream dispatch, so the upstream's own
# time is excluded). The tenant is throwaway and its master key is bogus, so the
# upstream answers 401: the probe costs nothing and never touches a real model.
#
# Fails when p50 >= 1000us (the sub-ms claim). p95 is a guard, not the claim: on
# the shared 1 vCPU host the tail is scheduler-bound (observed 1.1-3.5ms), while
# a cache regression measured 6-28ms. Runs from the deploy job on every
# deployment (scripts/migrate.sh has already added `proxy_us`).
#
# Usage (on the host):   bash ./scripts/perf-check.sh
# Environment:
#   PERF_BASE        proxy base URL            (default http://127.0.0.1:${PORT:-8080})
#   PERF_REQUESTS    measured requests         (default 40)
#   PERF_P50_US      p50 budget in µs          (default 1000 — the claim)
#   PERF_P95_US      p95 guard in µs           (default 5000 — see below)
#   POSTGRES_CONTAINER                         (default inferon-postgres-1)

set -euo pipefail
cd "$(dirname "$0")/.."

BASE="${PERF_BASE:-http://127.0.0.1:${PORT:-8080}}"
CONTAINER="${POSTGRES_CONTAINER:-inferon-postgres-1}"
REQUESTS="${PERF_REQUESTS:-40}"
P50_BUDGET="${PERF_P50_US:-1000}"
P95_BUDGET="${PERF_P95_US:-5000}"

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

# Point the throwaway tenant at a dead local port so each probe fails in
# microseconds (connection refused) instead of leaving the host. The key
# ciphertext is copied from an existing provider so it decrypts with this
# deployment's key; the row is deleted with the tenant. With no provider to
# copy, the tenant falls back to the shared upstream (a slow, free 401).
if [ "$(psql "SELECT count(*) FROM providers;")" != "0" ]; then
    psql "INSERT INTO providers
            (tenant_id, kind, name, base_url, key_ciphertext, key_hint, metadata,
             spend_limit_cents, is_default)
          SELECT '$TENANT', kind, 'perf-check', 'http://127.0.0.1:9/v1',
                 key_ciphertext, key_hint, metadata, 0, true
          FROM providers ORDER BY created_at LIMIT 1;" >/dev/null
fi

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
# Sequential on purpose: the claim is per-request wire latency, not throughput.
# The burst finishes well inside the 5 s config TTL, so every sample is warm.
for _ in $(seq 1 "$REQUESTS"); do
    curl -sS -o /dev/null --max-time 10 \
        "$BASE/v1/chat/completions" \
        -H "Authorization: Bearer $KEY" \
        -H 'content-type: application/json' -d "$BODY" || true
done
# Telemetry flushes every 500 ms.
sleep 2

# `psql -tA` separates fields with '|', not whitespace.
IFS='|' read -r COUNT P50 P95 <<<"$(psql "SELECT count(*),
        COALESCE(round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us)), 0),
        COALESCE(round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us)), 0)
    FROM traces WHERE tenant_id = '$TENANT' AND proxy_us IS NOT NULL;")"

# Never pass vacuously: a parse failure must fail the check.
for value in "$COUNT" "$P50" "$P95"; do
    case "$value" in
        ''|*[!0-9]*)
            echo "perf-check: could not parse the measurement (count=${COUNT:-} p50=${P50:-} p95=${P95:-})" >&2
            exit 1
            ;;
    esac
done

echo "warm proxy overhead: ${COUNT} requests · p50 ${P50}us · p95 ${P95}us"
echo "claim:           p50 < ${P50_BUDGET}us"
echo "p95 guard:       < ${P95_BUDGET}us  (1-vCPU scheduler jitter; a broken cache measured 6-28ms)"

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
