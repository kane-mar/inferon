## ITEM-004: Auth & Tenant Context (Bearer Key Swap)

**As a** tenant
**I want** my API key to authenticate against the proxy and identify my tenant context
**So that** spend is attributed to my tenant and my upstream credentials are never exposed to my developers

**Acceptance Criteria:**
1. `Authorization: Bearer <key>` is required; missing header → 401
2. Key is SHA-256 hashed and looked up against `tenants.api_key_hash`; unknown key → 401
3. The upstream `Authorization` header is swapped to the tenant's master upstream key before forwarding
4. Tenant ID is attached to every trace written by telemetry (ITEM-005)
5. Raw keys are never stored or logged — only the SHA-256 hash

**Technical Notes:**
- Lookup via sqlx against Postgres (or a local in-process cache; Redis cache optional later)
- DeepSeek is the default upstream — confirm the key column/mechanism for DeepSeek credentials (spec schema has upstream_openai_key / upstream_anthropic_key; DeepSeek key storage TBD)
- Source: implementation spec auth flow (see wiki `inferon-slice-engineering-spec.md` → Slice 1)
