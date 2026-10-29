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

## Close-out (2026-10-29)

- **Keyless, end to end** (AC1): `providers::requires_key(kind)` lets `local` be
  created and probed with a base URL only. `auth_headers` returns **no** header
  for a keyless local provider, and every upstream request — chat, guard replay
  and Anthropic — now goes through one `proxy::provider_request` helper that
  uses it, so the inference path no longer hardcodes `Bearer {key}` either
  (found live: the probe was keyless but the request still sent `Bearer `).
  A local provider that *does* have a key still sends it.
- **The flow** (AC2): `local` has its own monogram/badge (no more grey `··`),
  a URL placeholder (`http://192.168.1.10:11434/v1`), and an in-place note:
  costs $0, the cloud must be able to reach the URL (TLS if public), only the
  exposed endpoint is touched. The key field is optional and says so.
- **Reachability on a cadence** (AC3): `run_reachability_pass` runs every 5
  minutes (`PROVIDER_PROBE_INTERVAL_SECS`), probes at most 50 due-first
  providers (`last_check_at NULLS FIRST`), records `last_check_ok`, and never
  calls inference. The manual Test button stays.
- **Truthful prices** (AC4): a local provider's models are shown as
  `$0 / $0`, `priced: true`, `local: true` in the model picker instead of a
  cloud price keyed off the model name or "unpriced"; the picker renders
  "Free — served on your own hardware". Traces were already $0 with the
  counterfactual preserved (ITEM-077).
- **Local**: Rust **337 → 344** tests (no-header forwarding; keyless create,
  keyless probe, cloud create still refused without a key; the sweep records
  up/down; local models shown free). Web `tsc`/eslint/i18n/unit green.
- **Live (2026-10-29)**, deploy `ac5e782`→`9c3eac2`, CI runs `154`/`156`:
  created a `local` provider for a real tenant with `key: ""` →
  `{"id": …}`; the unsaved probe returned `{"ok":true,"models":["llama3:8b",
  "qwen3:8b"]}`; `GET …/models` showed both at `$0` with `local: true`; a real
  `/v1/chat/completions` for `qwen3:8b` returned 200 and the endpoint's log
  showed `auth= None` (the pre-fix request logged `auth= 'Bearer '`). The trace
  carries the local `provider_id` and `upstream_cost_usd = 0.000000`. Stopping
  the endpoint at 13:39:19 flipped `last_check_ok` to `false` at the next tick
  (**13:41:12**) with no failed tenant request. The provider was then removed
  and the temporary key revoked.
- Not done on purpose: model discovery is not triggered by the save — the
  dashboard's Models panel (or `GET …/providers/:id/models`) does it, as for
  every other kind.
