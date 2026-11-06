> **Delivered 2026-11-06.** `#[serde(default)]` on the provider body's `key`, so absent
> means empty; `requires_key(kind)` still refuses an empty key on the four keyed kinds with
> the typed `400 invalid_key`. Verified live on the rebuilt proxy: a `local` provider is
> created without the field, and a `deepseek` one without it gets the typed refusal. Two
> tests, 565 in total, clippy clean.

## ITEM-129: Creating a keyless `local` provider fails unless the body carries `key`

**As an** API client adding a model on my own network
**I want** to omit the credential entirely
**So that** a keyless provider does not require me to send an empty string

**Found 2026-11-06 while writing the ITEM-118 provider guide.** `POST
/v1/dashboard/api/providers` with `{"kind":"local","base_url":"…"}` fails to deserialise,
and the response is a raw serde message rather than our typed error shape:

```text
Failed to deserialize the JSON body into the target type: missing field `key` at line 1 column 70
```

Sending `"key": ""` succeeds. The dashboard always sends `""`, so nobody hits this through
the UI — but the API contract says a local provider needs no key while the type says it
does, and a client author following the guides will hit it.

**Fix.** `#[serde(default)]` on the field, so a missing key means the empty string, plus
the existing `requires_key(kind)` check deciding whether empty is acceptable — which
already refuses an empty key for the four keyed kinds with a typed `invalid_key`.

**Acceptance Criteria**
1. `{"kind":"local","base_url":"…"}` creates a provider (no `key` field).
2. `{"kind":"deepseek","base_url":"…"}` (no key) answers the typed `400 invalid_key`
   instead of a serde error.
3. A test covers both, and the guide's JSON example drops the empty `key`.
4. No other provider body field behaves this way.

**Source:** ITEM-118 evidence run, 2026-11-06.
