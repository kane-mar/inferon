#!/usr/bin/env bash
# ITEM-014b/014c: durable (always-on) deployment of the Inferon stack behind
# Caddy TLS. Unlike docker-host.sh this host is KEPT RUNNING (~$5-6/mo) — the
# deploy is repeatable: re-run to rebuild/update the stack on the same host.
#
# Usage:
#   deploy-durable.sh up      # provision the durable host + deploy the stack
#   deploy-durable.sh redeploy # rebuild + restart the stack on the existing host
#   deploy-durable.sh status  # compose ps + public endpoint probes
#   deploy-durable.sh teardown # destroy the host (stops billing)
#
# Env: VULTR_API_KEY, VULTR_REGION, VULTR_PLAN, VULTR_SSH_KEY_ID,
#      OPERATOR_API_KEY (optional; generated + saved if unset),
#      INFERON_HOST (public TLS hostname; saved after first deploy),
#      INFERON_DURABLE_STATE (default ~/.cache/inferon-durable)

set -euo pipefail

API="https://api.vultr.com/v2"
REGION="${VULTR_REGION:-ewr}"
PLAN="${VULTR_PLAN:-vc2-1c-1gb}"
OS_ID=2284 # Ubuntu 24.04 LTS x64
SSH_KEY_ID="${VULTR_SSH_KEY_ID:-6f4371fa-5af1-4363-8a75-afdd1ce54804}"
SSH_KEY="$HOME/.ssh/inferon_deploy"
STATE_DIR="${INFERON_DURABLE_STATE:-$HOME/.cache/inferon-durable}"
STATE="$STATE_DIR/state"
OP_FILE="$STATE_DIR/operator_key"
# Vultr firewall group to attach (web ingress). Defaults to the account's
# "inferon-durable: web ingress" group when set by the operator.
FIREWALL_GROUP="${VULTR_FIREWALL_GROUP:-}"

: "${VULTR_API_KEY:?set VULTR_API_KEY}"
[ -f "$SSH_KEY" ] || { echo "missing ssh key: $SSH_KEY" >&2; exit 1; }
mkdir -p "$STATE_DIR"

field() { sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p" | head -1; }

instance_state() {
    curl -fsS "$API/instances/$1" -H "Authorization: Bearer $VULTR_API_KEY"
}

up() {
    if [ -f "$STATE" ]; then
        echo "durable host already exists: $(cat "$STATE")" >&2
        echo "run: $0 redeploy" >&2
        exit 1
    fi
    local resp id ip
    echo "provisioning permanent $PLAN @ $REGION ..."
    resp=$(curl -fsS -X POST "$API/instances" \
        -H "Authorization: Bearer $VULTR_API_KEY" \
        -H "Content-Type: application/json" \
        -d "{\"region\":\"$REGION\",\"plan\":\"$PLAN\",\"os_id\":$OS_ID,\"label\":\"inferon-durable\",\"hostname\":\"inferon-durable\",\"sshkey_id\":[\"$SSH_KEY_ID\"]}")
    id=$(printf '%s' "$resp" | field id)
    [ -n "$id" ] || { echo "provision failed" >&2; exit 1; }
    for _ in $(seq 1 60); do
        local json status
        json=$(instance_state "$id")
        status=$(printf '%s' "$json" | field status)
        ip=$(printf '%s' "$json" | field main_ip)
        if [ "$status" = "active" ] && [ -n "$ip" ] && [ "$ip" != "0.0.0.0" ]; then
            printf '%s %s\n' "$id" "$ip" > "$STATE"
            echo "ACTIVE $id $ip"
            if [ -n "$FIREWALL_GROUP" ]; then
                curl -fsS -X PATCH "$API/instances/$id" \
                    -H "Authorization: Bearer $VULTR_API_KEY" \
                    -H "Content-Type: application/json" \
                    -d "{\"firewall_group_id\":\"$FIREWALL_GROUP\"}" >/dev/null
                echo "attached firewall group $FIREWALL_GROUP"
            else
                echo "WARNING: no VULTR_FIREWALL_GROUP set — all ports are open" >&2
            fi
            deploy
            return 0
        fi
        sleep 10
    done
    echo "timed out waiting for activation" >&2
    exit 1
}

bootstrap_docker() {
    local ip=$1
    echo "bootstrapping docker on $ip..."
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "root@$ip" \
        "export DEBIAN_FRONTEND=noninteractive && apt-get update -qq && apt-get install -y -qq docker.io docker-compose-v2 >/dev/null 2>&1 && systemctl enable --now docker >/dev/null 2>&1 && docker --version | head -1"
}

wait_ssh() {
    local ip=$1
    for _ in $(seq 1 15); do
        if ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no -o ConnectTimeout=10 "root@$ip" true 2>/dev/null; then
            return 0
        fi
        sleep 5
    done
    echo "ssh not reachable on $ip" >&2
    return 1
}

# Which hostname serves the public TLS endpoint. INFERON_HOST env wins;
# otherwise the value persisted at the first deploy. Always persisted so
# later redeploys/status reuse it without the env var.
resolve_hostname() {
    if [ -z "${INFERON_HOST:-}" ] && [ -f "$STATE_DIR/hostname" ]; then
        INFERON_HOST=$(cat "$STATE_DIR/hostname")
    fi
    [ -n "${INFERON_HOST:-}" ] || { echo "INFERON_HOST is required (the public hostname, e.g. INFERON_HOST=api.example.sh)" >&2; exit 1; }
    printf '%s\n' "$INFERON_HOST" > "$STATE_DIR/hostname"
}

deploy() {
    local ip id
    read -r id ip < "$STATE"
    resolve_hostname
    echo "waiting for ssh on $ip..."
    wait_ssh "$ip"
    bootstrap_docker "$ip"

    # Operator key: env wins; otherwise generate once and save (mode 600).
    if [ -z "${OPERATOR_API_KEY:-}" ] && [ ! -f "$OP_FILE" ]; then
        umask 077
        OPERATOR_API_KEY="sk-op-$(openssl rand -hex 16)"
        printf '%s\n' "$OPERATOR_API_KEY" > "$OP_FILE"
    elif [ -f "$OP_FILE" ] && [ -z "${OPERATOR_API_KEY:-}" ]; then
        OPERATOR_API_KEY=$(cat "$OP_FILE")
    fi

    bundle_upload "$ip"
    remote_deploy "$ip"
    remote_health "$ip"
    if [ ! -f "$OP_FILE" ]; then
        echo "OPERATOR_API_KEY (saved to $OP_FILE): $OPERATOR_API_KEY"
    fi
    echo "deployed. stack: docker compose ps (host $ip)"
    echo "public endpoint: https://$INFERON_HOST (point an A record at $ip to go live)"
}

# Bundle the runtime inputs (same file list as the CI/CD workflow) and ship it.
bundle_upload() {
    local ip=$1 bundle
    bundle=$(mktemp /tmp/inferon-durable.XXXXXX)
    tar czf "$bundle" \
        --exclude='web/node_modules' --exclude='web/.next' \
        Cargo.toml Cargo.lock Dockerfile docker-compose.yml Caddyfile .dockerignore .env.example src scripts db web
    scp -q -i "$SSH_KEY" -o StrictHostKeyChecking=no "$bundle" "root@$ip:/tmp/durable.tgz"
    rm -f "$bundle"
}

# Non-secret + optional env passed to compose. Values only travel over the ssh
# command line (same channel as the operator key).
compose_env() {
    local out="INFERON_HOST='$INFERON_HOST' OPERATOR_API_KEY='$OPERATOR_API_KEY' NEXT_PUBLIC_APP_URL='https://$INFERON_HOST'"
    local var val
    for var in GITHUB_CLIENT_ID GITHUB_CLIENT_SECRET SESSION_SECRET UPSTREAM_MASTER_KEY \
               OPERATOR_GITHUB_LOGINS ALERT_WEBHOOK_URL UPSTREAM_URL; do
        val="${!var:-}"
        [ -n "$val" ] && out="$out $var='$val'"
    done
    printf '%s' "$out"
}

# Deploy on the host: extract, bring Postgres up, apply db/init (idempotent —
# the entrypoint only auto-applies on a fresh volume), then rebuild + start the
# full prod stack.
remote_deploy() {
    local ip=$1 env_args
    env_args=$(compose_env)
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "root@$ip" \
        "mkdir -p /opt/inferon && tar xzf /tmp/durable.tgz -C /opt/inferon && chmod -R a+rX /opt/inferon \
         && cd /opt/inferon && chmod +x scripts/*.sh \
         && docker compose up -d postgres redis \
         && ./scripts/migrate.sh \
         && export $env_args \
         && docker compose --profile prod up -d --build \
         && docker compose --profile prod up -d --force-recreate caddy"
}

remote_health() {
    local ip=$1
    for _ in $(seq 1 60); do
        if ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "root@$ip" "curl -fsS http://127.0.0.1:8080/healthz >/dev/null 2>&1 && curl -fsS http://127.0.0.1:3000/api/health >/dev/null 2>&1"; then
            echo "stack healthz (proxy + web) OK"
            return 0
        fi
        sleep 5
    done
    echo "health check failed" >&2
    return 1
}

redeploy() {
    [ -f "$STATE" ] || { echo "no durable host state — run: $0 up" >&2; exit 1; }
    local ip id
    read -r id ip < "$STATE"
    resolve_hostname
    wait_ssh "$ip"
    bootstrap_docker "$ip"
    OPERATOR_API_KEY=$(cat "$OP_FILE" 2>/dev/null || echo "${OPERATOR_API_KEY:-}")
    bundle_upload "$ip"
    remote_deploy "$ip"
    remote_health "$ip"
    echo "redeployed"
}

status() {
    [ -f "$STATE" ] || { echo "no durable host state" >&2; exit 1; }
    local ip id
    read -r id ip < "$STATE"
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "root@$ip" \
        "cd /opt/inferon && docker compose --profile prod ps --format '{{.Name}} {{.Status}}'"
    echo "healthz (plain http): $(curl -sS -o /dev/null -w '%{http_code}' "http://$ip/healthz" 2>/dev/null || echo unreachable)"
}

teardown() {
    [ -f "$STATE" ] || { echo "no durable host state" >&2; exit 1; }
    local id
    read -r id _ < "$STATE"
    curl -fsS -X DELETE "$API/instances/$id" -H "Authorization: Bearer $VULTR_API_KEY" >/dev/null
    rm -f "$STATE" "$OP_FILE"
    echo "destroyed $id — billing stopped"
}

case "${1:-}" in
    up) up ;;
    redeploy) redeploy ;;
    status) status ;;
    teardown) teardown ;;
    *) echo "usage: $0 up|redeploy|status|teardown" >&2; exit 1 ;;
esac
