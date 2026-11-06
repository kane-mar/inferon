> **Delivered 2026-11-06.** Route resolution is traced on both surfaces (class
> `inferon_4xx`), and every generated refusal returns with the id of the row it wrote:
> `no_provider`, `surface_mismatch`, `model_disabled`, `rate_limited`, the budget
> refusal and `upstream_error`. The Anthropic handler starts its clock before routing so
> a refusal is timed. 553 tests, clippy clean. Verified on the rebuilt proxy against the
> live host: a workspace with no provider answers `400` with a trace id whose row is
> `status=400 class=inferon_4xx` (Quality: 1 request, 1 error, `inferon_4xx`); the
> Anthropic surface does the same; a closed-port provider answers `502` with a header
> and class `transport`; a key with `rpm_limit=1` answers `429` with a header and class
> `rate_limited`. The two ITEM-117 pages lost their caveat in the same change.

## ITEM-123: A refusal you can point at — trace ids on gateway errors, and no_provider is not traced

**As a** developer whose request was refused, and as the person answering their message
**I want** the refusal to appear in the reports and to name itself
**So that** "it says no_provider" is one dashboard lookup instead of a guess

**Found 2026-11-05 while writing ITEM-117 against the live host.** Two gaps on the same
paths, both verified:

1. **`no_provider` writes no trace at all.** A freshly provisioned workspace answers
   `400 no_provider` on `/v1/chat/completions` and `/v1/messages`; three seconds later
   the workspace has zero rows in `traces` and its Quality report reads `requests: 0`.
   The route resolution fails before the trace sink is called. The most common first
   failure is therefore invisible in the dashboard.
2. **Errors the gateway generates carry no `x-inferon-trace-id`.** Measured on the live
   host: `400 no_provider`, `429 rate_limited` and `502 upstream_error` come back
   without the header, while a relayed provider error (`429`, `500`) does carry it. The
   rate-limited request *is* traced (`class=rate_limited`), and the transport failure is
   traced (`class=transport`), so the row exists and cannot be pointed at.

The guides now say so in plain words (ITEM-117, `no_provider` and `429` pages). That is
honest but it is a workaround: a support answer should be able to name a trace id.

**Acceptance Criteria**
1. `400 no_provider` (both surfaces and the model-not-served case) is recorded as a
   trace row with its error class, and the client can find it by `x-inferon-trace-id`.
2. Every response the gateway generates — `no_provider`, `surface_mismatch`,
   `rate_limited`, `upstream_error`, disabled-model refusals — carries
   `x-inferon-trace-id`, matching the row it wrote.
3. Relayed provider errors keep their current behaviour (body and status unchanged,
   header present).
4. Unit tests cover each generated-error path: header present, one trace row, correct
   class. A test reads the router's error constructors rather than a real socket.
5. Live: on a workspace with no provider, the refusal carries a trace id and Quality
   counts it; on a key with `rpm_limit=1`, the second request carries the header and
   `rate_limited` appears in **Errors by cause**.
6. The `no_provider` and `429` troubleshooting pages lose their "no trace id" caveat in
   the same change.

**Technical Notes**
- The header is added by `proxy/response.rs::decorate`; the generated refusals return
  before it. The `RequestMeta` (and its id) already exists at those points.
- Route resolution returns `Err(Response)` from `route.rs`, so the fix is to trace and
  decorate at the call site, not to teach the resolver about tracing.

**Source:** live verification during ITEM-117, 2026-11-05.
