## ITEM-002: Postgres Schema (tenants + traces)

**As a** platform engineer
**I want** the `tenants` and `traces` tables migrated automatically on container start
**So that** auth (ITEM-004) and telemetry (ITEM-005) have durable, queryable storage from day one

**Acceptance Criteria:**
1. Migration creates `tenants` (id UUID `uuid_generate_v4()`, api_key_hash VARCHAR(64) UNIQUE NOT NULL, upstream_openai_key TEXT, upstream_anthropic_key TEXT, created_at) per the implementation spec
2. Migration creates `traces` (id, tenant_id FK ON DELETE CASCADE, request_model, system_prompt_hash, prompt_tokens, completion_tokens, time_to_first_token_ms, total_duration_ms, upstream_cost_usd NUMERIC(10, 6), waste_detected JSONB, status_code, raw_prompt, raw_completion, created_at)
3. Indexes: `idx_traces_tenant_created` on (tenant_id, created_at DESC) and `idx_traces_prompt_hash` on system_prompt_hash
4. Migration is idempotent (`CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`) and runs automatically on Postgres container start
5. Runs against the project decision: Postgres only (no SQLite)

**Technical Notes:**
- Source: `inferon-complete-implementation-spec.md` schema.sql (see wiki `aio-proxy-overview.md` → Control Plane Data Schema)
- No data migrations needed yet; keep SQL in `db/migrations/`

## Verification (2026-08-30) — on-demand Vultr host, postgres:16-alpine
- Migration db/init/001_init.sql auto-applied via docker-entrypoint-initdb.d on first init ✅
- tenants + traces created with all expected columns/types ✅ (test-schema.sh column checks)
- traces.tenant_id FK ON DELETE CASCADE ✅; tenants.api_key_hash UNIQUE ✅
- Indexes idx_traces_tenant_created + idx_traces_prompt_hash ✅
- Idempotent re-apply (no-op on second run) ✅
- scripts/test-schema.sh: ALL SCHEMA CHECKS PASSED; full stack smoke.sh green after ✅
