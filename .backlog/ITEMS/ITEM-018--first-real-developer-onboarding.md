## ITEM-018: First Real Developer Onboarding

**As a** product owner
**I want** a real external developer pasting the public baseURL + a tenant key into their SDK
**So that** the product goal ("in front of real developers") is actually met with someone other than us

**Acceptance Criteria:**
1. A "For developers" section in the README documents: baseURL `https://inferon.agilefederation.com/v1`, how a tenant key is issued (seed-tenant.sh runbook, delivered out-of-band), the supported surface (chat completions incl. streaming, dashboard URLs), and a 2-minute OpenAI-SDK example
2. A second real tenant is minted via the documented path and handed to a real developer (outside our team); the key round-trip is verified unprinted
3. The developer's first real request through the public URL streams a 200; a trace lands in Postgres and their dashboard shows it — tenant-scoped (no visibility into ours)
4. Operator views list both tenants cleanly; cross-tenant access still returns 401/403
5. Anything the real integration surfaces (SDK quirks, missing endpoints, unclear docs) is captured as new backlog items rather than silently worked around

**Technical Notes:**
- Live surface already verified (ITEM-014): healthz, chat, tenant/operator dashboards over the public HTTPS URL
- This is the first demand-side validation of the drop-in claim — the point is to learn, not to polish
- Likely to surface ITEM-019/020-shaped follow-ups; capture them, don't scope-creep this item
