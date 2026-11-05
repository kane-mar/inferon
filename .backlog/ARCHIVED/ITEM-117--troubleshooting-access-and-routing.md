> **Delivered 2026-11-05.** `guides/troubleshooting/{401-403,no-provider,too-many-requests,upstream-5xx}.md`,
> each in the inherited shape, each reproduced on the live host: a real `rpm_limit=1`
> 429 with `Retry-After: 53` and the `X-RateLimit-*` headers; a relayed upstream 429
> and 500 from a mock provider on the compose network; a chain step handling
> `rate_limited`; a step handling `5xx` that then fails, where the client receives the
> second step's error; a `502 upstream_error` for a closed port; a member's `403
> insufficient_role`; a revoked key's `401`. Two gaps found on the way are captured as
> ITEM-123.

## ITEM-117: Troubleshooting — access and routing (401, no_provider, 429, 5xx)

**As a** developer whose request just failed
**I want** the symptom, what it means, the fix in a few steps, and how to confirm it
worked
**So that** I can unblock myself at 2am with no support channel

**Slices from ITEM-113.** Four pages under `guides/troubleshooting/`, one per
symptom. Every page uses the inherited shape (Symptom → What it means → Fix → How to
confirm it worked → When to stop and ask) and the `Last verified` footer, and must
name the endpoint, dashboard page, log line or table where the evidence is.

1. `401-403.md` — missing key, revoked key, wrong workspace, `invalid_api_key`.
2. `no-provider.md` — the workspace has no provider and no platform key. The most
   likely first support question; guide 1 must link here.
3. `too-many-requests.md` — the provider rate-limited us vs our own per-key RPM/TPM.
   How to tell the two apart from the quality report and the trace.
4. `upstream-5xx.md` — which fallback step ran, what each trigger covers, and what an
   exhausted chain returns to the client.

**Acceptance Criteria**
1. Each page is reproduced from a real failure on the live host (or the local stack
   with the mock upstream), with the actual status code and JSON body quoted.
2. Each page is 3–6 fix steps, not prose; it links to the configuration page that
   causes the error once that page exists (ITEM-118/119) and back.
3. `python3 scripts/check-guides.py` passes, including the five-section check.
4. Live pages: the four files are on the durable host and every quoted command was
   run on the date in the footer.

**Source:** ITEM-113 breakdown, 2026-11-05.
