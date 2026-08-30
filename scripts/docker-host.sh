#!/usr/bin/env bash
# On-demand Vultr Docker host for Inferon verification.
# Spawn a VPS, verify the deployment seam, destroy it when done.
#
# Usage:
#   docker-host.sh up       # provision VPS, wait for activation, print IP
#   docker-host.sh down     # destroy the VPS (by instance id from state)
#   docker-host.sh verify   # up -> deploy bundle -> scripts/smoke.sh -> down
#
# Env: VULTR_API_KEY (required), VULTR_REGION, VULTR_PLAN, VULTR_SSH_KEY_ID

set -euo pipefail

API="https://api.vultr.com/v2"
REGION="${VULTR_REGION:-ewr}"
PLAN="${VULTR_PLAN:-vc2-1c-1gb}"
OS_ID=2284 # Ubuntu 24.04 LTS x64
SSH_KEY_ID="${VULTR_SSH_KEY_ID:-6f4371fa-5af1-4363-8a75-afdd1ce54804}"
SSH_KEY="$HOME/.ssh/inferon_deploy"
STATE="${INFERON_HOST_STATE:-$HOME/.cache/inferon-docker-host}"

: "${VULTR_API_KEY:?set VULTR_API_KEY}"
[ -f "$SSH_KEY" ] || { echo "missing ssh key: $SSH_KEY" >&2; exit 1; }

field() { sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p" | head -1; }

instance_state() {
    curl -fsS "$API/instances/$1" -H "Authorization: Bearer $VULTR_API_KEY"
}

up() {
    echo "provisioning $PLAN @ $REGION ..."
    local resp id
    resp=$(curl -fsS -X POST "$API/instances" \
        -H "Authorization: Bearer $VULTR_API_KEY" \
        -H "Content-Type: application/json" \
        -d "{\"region\":\"$REGION\",\"plan\":\"$PLAN\",\"os_id\":$OS_ID,\"label\":\"inferon-docker\",\"hostname\":\"inferon-docker\",\"sshkey_id\":[\"$SSH_KEY_ID\"]}")
    id=$(printf '%s' "$resp" | field id)
    [ -n "$id" ] || { echo "provision failed" >&2; exit 1; }

    for _ in $(seq 1 60); do
        local json status ip
        json=$(instance_state "$id")
        status=$(printf '%s' "$json" | field status)
        ip=$(printf '%s' "$json" | field main_ip)
        if [ "$status" = "active" ] && [ -n "$ip" ] && [ "$ip" != "0.0.0.0" ]; then
            printf '%s %s\n' "$id" "$ip" > "$STATE"
            echo "ACTIVE $id $ip"
            return 0
        fi
        sleep 10
    done
    echo "timed out waiting for activation" >&2
    exit 1
}

down() {
    local id
    id=$(awk '{print $1}' "$STATE" 2>/dev/null || true)
    [ -n "$id" ] || { echo "no host in state ($STATE)" >&2; exit 1; }
    for _ in 1 2 3 4 5; do
        local code
        code=$(curl -sS -o /dev/null -w "%{http_code}" -X DELETE "$API/instances/$id" -H "Authorization: Bearer $VULTR_API_KEY" || true)
        [ "$code" = "204" ] || [ "$code" = "404" ] && break
        sleep 5
    done
    rm -f "$STATE"
    echo "destroyed $id"
}

deploy() {
    local ip bundle
    ip=$(awk '{print $2}' "$STATE")
    bundle=$(mktemp /tmp/inferon-deploy.XXXXXX.tgz)
    tar czf "$bundle" Cargo.toml Cargo.lock Dockerfile docker-compose.yml .dockerignore .env.example src scripts
    scp -q -i "$SSH_KEY" -o StrictHostKeyChecking=no "$bundle" "root@$ip:/tmp/deploy.tgz"
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "root@$ip" \
        "mkdir -p /opt/inferon && tar xzf /tmp/deploy.tgz -C /opt/inferon && cd /opt/inferon && chmod +x scripts/smoke.sh && ./scripts/smoke.sh"
    rm -f "$bundle"
}

verify() {
    trap down EXIT
    up
    deploy
}

case "${1:-}" in
    up) up ;;
    down) down ;;
    verify) verify ;;
    *) echo "usage: $0 up|down|verify" >&2; exit 1 ;;
esac
