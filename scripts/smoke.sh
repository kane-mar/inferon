#!/usr/bin/env bash
# Deployment seam verification for ITEM-001 (runs on a host with Docker).
# Validates compose config, brings the stack up, waits for proxy health.

set -euo pipefail
cd "$(dirname "$0")/.."

docker compose config --quiet
docker compose up -d --build
trap 'docker compose down' EXIT

for i in $(seq 1 30); do
    if curl -fsS "http://127.0.0.1:${PORT:-8080}/healthz" >/dev/null 2>&1; then
        echo "proxy healthy after ${i}s"
        exit 0
    fi
    sleep 1
done

echo "proxy did not become healthy within 30s" >&2
exit 1
