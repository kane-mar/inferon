## ITEM-027: Schema Guard — Activation + Regression Safety (Slice 4 · was 021e)

**As a** product owner
**I want** the guard on by default for cheap-tier structured requests without ever regressing existing tenants
**So that** the safety net ships without a migration scare

**Acceptance Criteria:**
1. Guard is active by default for requests that carry a JSON-schema contract and route to the cheap tier; a per-tenant opt-out flag lives on `tenant_configurations`
2. Pure-passthrough requests (no `response_format` or not routed) show zero behavior change — verified by the existing smoke surface
3. `scripts/smoke.sh` gains a guarded-failure scenario: a dead/lying guarded upstream forces a fallback and the client still receives a valid 200
4. Hot-path budget verified on the durable host: <5ms added on guarded traffic (Slice 4 latency target), none on unguarded; no TTFT regression on the happy path beyond the buffered flush

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021e — the guard item's completion slice
- Depends on ITEM-023..026
- Open decision from parent: default scope = all cheap-tier routes vs opt-in until ITEM-018 brings real traffic shapes — the smoke scenario above assumes default-on
