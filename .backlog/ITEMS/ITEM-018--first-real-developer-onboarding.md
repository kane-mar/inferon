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

## Breakdown note (2026-09-06 refinement)

ITEM-018's remaining body is a single PO-gated human step, so it is kept whole
rather than split into fake sub-items. Its tail decomposes as:

1. **PO action (this item's gate)**: name the external developer; hand over the
   staged bundle (`~/.cache/inferon-durable/second-real-developer.key` +
   `.handoff.txt`) — the key itself already round-trips and authenticates.
2. **Next pull once the PO acts**: run the developer's first real request with
   them + capture AC5 findings (SDK quirks, missing endpoints, doc gaps) as new
   items — the observation loop is specified in the verification record above.

With the accounts horizon refined (ITEM-034..040), this out-of-band minting path
is superseded for any FUTURE developer by self-serve signup (ITEM-036), and the
migration is completed by ITEM-040. The staged second-real-developer key remains
valid as an ordinary API key (ITEM-034 rows) — no migration needed for it.
Ordering decision for the PO: hand the staged key out now (fastest real-traffic
data point, feeds ITEM-036's signup UX) or let that developer self-serve once
ITEM-036 ships — see REFINEMENT_LOG 2026-09-06.

## Close-out (ITEM-040, 2026-09-07)

The out-of-band human-onboarding path this item exercised is CLOSED: self-serve
sign-in (`/auth/login` → auto-provisioned tenant + owner + first key) is the
only documented onboarding for humans (ITEM-036 delivered; seed-tenant.sh is
now operator/machine-only). The staged second-real-developer key stays valid as
an ordinary api_keys row (ITEM-034 semantics) — no data migration.

What remains is a PO action, not build work: either hand the staged key to a
named developer for the fastest real-traffic data point (AC5 capture loop
above), or leave it — that developer can simply self-serve. Archive this item
once the PO makes that call.

### Update (2026-10-04, ITEM-041 R2 rebuild)

The staged second-real-developer key was **lost with the deleted host** (its
tenant row is gone); the local file is retired as `*.lost-2026-10-04`. The path
stays closed — a developer simply self-serves (once the GitHub OAuth app env is
set on the host). The freshly rebuilt host holds one re-minted tenant
(`first-real-developer.key`) on the shared upstream key.
