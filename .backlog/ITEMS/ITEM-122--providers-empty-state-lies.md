## ITEM-122: Providers empty state promises the shared platform key

**As a** developer who has just signed up
**I want** the Providers page to tell the truth about where my requests will go
**So that** I add a provider instead of waiting for a request that cannot work

**Found 2026-11-05 while verifying ITEM-116 on the live host.** With no provider, the
Providers page says:

> "No provider yet — traffic uses the shared platform key."

That is true only for an operator-seeded workspace. A self-serve sign-up has no
`tenants.upstream_openai_key`, so the first request is refused:

```
$ curl -s … /v1/chat/completions -H "Authorization: Bearer sk-inferon-…" …
{"error":{"message":"no upstream provider configured — add one in Dashboard → Providers","type":"no_provider"}} [400]
```

Evidence: a freshly provisioned tenant (no provider, no master key) answers `400
no_provider` on `/v1/chat/completions` and `/v1/messages`, and `GET /v1/models`
returns an empty list. `src/proxy/route.rs::resolve_route` only falls back to
`tenant.upstream_openai_key`; the platform env key is not a pass-through (README
already says so).

**Fix.** Replace the empty-state string in `en.json` and `fr.json`
(`ProvidersPage.empty`) with a statement that holds for both shapes, e.g. "No
provider yet — add one so requests have somewhere to go." Nothing else changes: no
schema, no endpoint, no behaviour.

**Acceptance Criteria**
1. The empty state no longer claims the platform key serves the workspace's traffic.
2. `en.json`/`fr.json` keep the same key; `i18n-check`, `typecheck`, unit tests and
   `next build` pass.
3. Live: the string renders on `/dashboard/providers` for a workspace with no
   provider, and the guides' `no_provider` wording matches it.
4. Docs updated in the same change if guide 1 quotes the page.

**Source:** live verification, 2026-11-05.
