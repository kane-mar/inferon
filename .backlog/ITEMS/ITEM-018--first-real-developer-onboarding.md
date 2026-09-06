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

## Verification record (2026-09-06)

- **AC1 (README)**: "For developers" section added — base URL, out-of-band key issuance
  (seed-tenant.sh runbook), supported surface (chat incl. streaming + dashboard URLs),
  Python OpenAI-SDK streaming/non-streaming example. Commit: README.md (see git log).
- **AC2 (mint, via documented path)**: tenant #2 "second-real-developer" minted on the
  durable host with `scripts/seed-tenant.sh`, master key read from the DB and never
  printed. Round-trip verified unprinted: `sha256(printed dev key)` == stored
  `api_key_hash` (ROUND-TRIP OK on host). Dev key saved 0600:
  `~/.cache/inferon-durable/second-real-developer.key` (+ `.handoff.txt` bundle with
  base URL + dashboard link) — **ready for PO handoff to an external developer.**
- **AC3 (first real request)**: streaming request as tenant #2 over the public URL
  `https://inferon.agilefederation.com/v1` returned SSE 200 (deepseek-v4-flash);
  non-stream 200. Trace landed: tenant #2's dashboard summary shows its 2 requests,
  $0.000014, tenant-scoped (`62ada0b8…`), zero visibility into tenant #1's rows.
- **AC4 (operator view + isolation)**: operator tenants API lists BOTH tenants
  (4d094a70… 3 reqs / 62ada0b8… 2 reqs). Tenant key → operator endpoint = 403;
  no key = 401; operator key on tenant endpoints = 403 (existing behavior).
- **AC5 (capture, don't work around)**: nothing broken surfaced in this integration
  pass; the one real finding is the *handoff itself is a PO action* — the key is
  staged 0600 for a real external developer who has not yet been named. When that
  developer runs their own SDK (any language), capture quirks here or as new items.

**Status**: all automatable ACs verified live (2026-09-06); **residual gate = PO handoff
of the staged key to a real developer outside the team** + that developer's first real
request. Item NOT archived until that human step completes.
