## ITEM-048: SDK onboarding flow ("point your SDK at it")

**As a** new developer who just signed in
**I want** a page that hands me the base URL, a key, and a copy-paste snippet
**So that** my first request lands in under two minutes

The landing page promises "point your existing OpenAI SDK at it". Today sign-up
shows a raw key once and nothing verifies the claim end-to-end. This clothes the
data plane in the onboarding the promise needs.

**Acceptance Criteria:**
1. After sign-up (and from the dashboard) a "Connect" page shows: the base URL, the
   tenant key (reveal-once, masked afterwards), and copy-paste snippets for OpenAI
   Python, Node, and cURL.
2. The page detects the tenant's **first trace** and flips to a success state
   ("first request received").
3. Links out to the model list (ITEM-044) and provider setup (ITEM-043).
4. Works for a tenant on the shared platform key *and* one with their own provider.

**Technical Notes:**
- Mostly frontend + one "has this tenant ever sent a request" query (the summary
  API already returns `first_trace_at`).
- Depends on the theme/design system pass so the page matches the site.
- Closes the loop with ITEM-018 (a real developer's first request).

**Source:** PO review 2026-10-06 — the landing claim is not backed by a flow.
