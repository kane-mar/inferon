> **Archived 2026-11-05 — split, not dropped.** The story and the page list below are
> the source; the work is ranked as ITEM-116 (docs skeleton + getting started,
> delivered), ITEM-117, ITEM-118, ITEM-119, ITEM-120 and ITEM-121. The acceptance
> criteria are inherited by those slices, with AC1 (published on the docs site)
> closing when ITEM-112's site exists.

## ITEM-113: Guide suite — using Inferon, configuring it, and fixing what breaks

**As a** workspace owner, developer or self-hoster
**I want** a task-shaped guide for the thing I am trying to do or the thing that just
broke
**So that** I can finish without reading the code or asking someone

**Context (2026-11-05, PO request).** There is one self-hosting guide and the README.
Nothing explains how to choose models, how to set a budget that does not break an
app, why the audit says "not cached", or what a `no_provider` error means. Every one
of those costs a support conversation today, and there is no support channel yet.

**The set to write** (grouped; each is its own page on the docs site, ITEM-112)

*Getting started*
1. Sign in, get a key, send the first request.
2. Move an existing OpenAI SDK to Inferon in five minutes (Python, Node, curl).
3. Add your own provider, and add a model that runs on your own network.

*Configuration, in the order a new workspace needs it*
4. Choosing models and tier pairs: what "expensive" and "cheap" mean here.
5. Setting budgets and rate limits that protect you without blocking real traffic.
6. Building a fallback chain that actually fires — and what each trigger covers.
7. Turning on the output checker (schema contract) without breaking existing calls.
8. Prompt caching: why the audit says "not cached" and what to change.
9. Context pruning and the comparison group: how to read whether it helped.
10. Storing payloads, retention, and the two PII modes.
11. Datasets → gold sets → evals: the loop from real traffic to a tested change.
12. Reading the cost audit's six findings, one by one.
13. Spend report: what a session is, and why the number differs from the provider's.

*Troubleshooting playbook — symptom, cause, fix, what to check*
14. `401` / `403`: missing key, revoked key, wrong workspace.
15. `no_provider`: the workspace has no provider and the platform key is not a
    pass-through (the most likely first support question).
16. `429`: the provider rate-limited us, or our own RPM/TPM limit fired — how to tell
    the two apart in the quality report.
17. `5xx` and the fallback chain: which step ran, and what an exhausted chain means.
18. Answers that are empty, refused, or cut short: `max_tokens`, reasoning tokens,
    and the truncation marker.
19. "Quality dropped after a downgrade": read the comparison group, then roll back.
20. A provider that tests as unreachable: local model, TLS, firewall, path prefix.
21. Dashboard empty although traffic is flowing: wrong key, capture off, window.
22. Audit shows `$0.00`: the model has no price in the table.
23. Sign-in loop or instant sign-out: `SESSION_SECRET` mismatch, cookie domain, OAuth
    callback URL.
24. `502` behind Caddy, or the web container healthy while the proxy is not.
25. Migrations did not apply after an upgrade, and the order that avoids it.
26. Retention or a purge removed the payloads you wanted to diff.

**Acceptance Criteria**
1. At least the 13 configuration/usage guides and the 13 troubleshooting entries above
   are published on the docs site, each as its own page.
2. Every troubleshooting page uses the same shape: **symptom → what it means → fix in
   3–6 steps → how to confirm it worked → when to stop and ask**. No page is a wall
   of prose.
3. Every command is copy-pasteable and was run against the live host or a local
   stack; the page states **Last verified <date> <commit>** so staleness is visible.
4. Each page names where to look: the endpoint, the dashboard page, the log line or
   the table involved. Configuration guides link to the exact control they change.
5. Reading level matches ITEM-110: mean sentence under 25 words, no undefined
   jargon, the technical term kept once it is defined ("workspace (tenant)").
6. Cross-links work both ways: a configuration guide links to the troubleshooting page
   for the error it can cause, and the troubleshooting page links back.
7. The docs link check (ITEM-112 AC7) passes; no page links to a file path a reader
   cannot open.
8. A new user can go from "signed in" to "first request answered" using only guide 1
   — tested with someone who has not seen the product.

**Technical Notes**
- Write them from the failure modes we have actually seen in the live host's logs and
  in the item history (`no_provider`, guard fallback, PII panic, cold-start latency,
  unpriced models), not from imagination.
- The troubleshooting entries are the raw material for the operator console's future
  "what to do" hints and for support replies; keep one canonical page per symptom so
  a support answer is a link.
- Screenshots go stale fastest; prefer a command plus its expected output, and a
  named dashboard control.

**Source:** PO request, 2026-11-05 ("create a number of guides to use inferon, and
help solve common configuration and routine problems").
