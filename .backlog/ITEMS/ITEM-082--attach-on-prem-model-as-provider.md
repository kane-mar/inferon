## ITEM-082: Attach an on-prem model as a provider (five-minute flow)

**As a** tenant with a model on my own network
**I want** to register it as a provider in a few fields
**So that** the cloud treats it like any other upstream — priced at zero, probed,
and visible in the dashboard — with no relay and no new transport

**Context (2026-10-28):** the capability already exists: any `base_url` works,
`POST /api/providers/test` probes an unsaved config, reachability is persisted
per provider (ITEM-051), and kind `local` prices traces at $0 with the full
counterfactual still attributed (ITEM-077). It is not a *flow* yet:

- `POST /providers` and `POST /providers/test` reject an empty key
  (`src/dashboard/providers.rs`), so a keyless Ollama/vLLM endpoint needs a fake
  key.
- The UI has no `local` styling — `KIND_STYLE` in `ProvidersPanel.tsx` has no
  `local` entry, so it renders the grey `··` fallback — and no `DEFAULT_BASE`
  hint or explanation of what "local" means.
- Reachability is only refreshed when someone clicks **Test connection**, so a
  dead on-prem box keeps its green badge until a real request fails.

**Acceptance Criteria**
1. A provider can be created with kind `local` using a **base URL only**: an
   empty key is accepted, and no `Authorization` header is sent for that kind.
   Probe and create behave identically.
2. The add form explains the local case in place — costs $0, must be reachable
   from the cloud (TLS if public), nothing else on the LAN is exposed — and
   `local` gets its own monogram/badge rather than the `openai-compatible`
   fallback.
3. Reachability is refreshed by a bounded periodic probe (the existing worker
   pattern, not a hot-path call), so a provider that goes dark shows
   **unreachable** before a request fails; the manual Test button remains.
4. A local provider's models are selectable as default/fallback like any other,
   and cost surfaces show `$0.00` for local traffic with the counterfactual
   still attributed (ITEM-077 semantics) — no "unpriced model" warnings.
5. Live: attach a real on-prem endpoint with no key → probe ok → a request
   returns 200 and the trace costs $0.000000 with the counterfactual recorded;
   stop the endpoint → the next periodic probe flips it to unreachable without
   any failed tenant request.

**Technical Notes**
- `auth_headers` already branches on kind; make the keyless case explicit rather
  than implicit (a `local` provider with an empty key must not send
  `Authorization: Bearer `).
- Periodic probe: reuse the scheduler that drives alerts; interval bounded and
  configurable, one probe per provider per interval, no request-path cost.
- No new transport, no relay, no credential fetched from the cloud. This is
  ITEM-051's mechanism with the friction removed.

**Source:** 2026-10-28 customer-hosted connectivity review (option A, made
five-minute).
