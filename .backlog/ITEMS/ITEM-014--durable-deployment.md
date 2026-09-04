## ITEM-014: Durable Deployment of api.inferon.dev

**As a** product owner
**I want** the proxy reachable at `https://api.inferon.dev/v1` on a permanent host
**So that** developers can paste the baseURL and get the Slice 1 experience for real

**Acceptance Criteria:**
1. A permanent (non-ephemeral) host runs the compose stack; traffic served over HTTPS on api.inferon.dev
2. Postgres data survives restarts and host rebuilds (backups or managed volume story)
3. Deploy is repeatable from a script (extend `scripts/docker-host.sh` or a new `deploy.sh`) — no hand-typed SSH
4. Tenants can be minted via a documented path (see ITEM-011)
5. Cost of the running host is understood and acknowledged (business decision — this is the first always-on spend)

**Technical Notes:**
- All verification so far used disposable Vultr hosts (destroyed after each item); nothing is deployed durably
- Domain/TLS: needs DNS for api.inferon.dev + a certificate story (Caddy/Traefik or a cloud LB)
- Break this down further when it reaches the top of the backlog (DNS, host provisioning, TLS, backups, tenant minting are candidate slices)
