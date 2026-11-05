## ITEM-116: Docs skeleton + getting started (index, first request, move an SDK)

**As a** developer who just signed in
**I want** one page that gets me to a working request, and one that shows me how to
point my existing code at Inferon
**So that** I do not need a support conversation to start

**Delivered (2026-11-05).** The first slice of ITEM-113. `guides/README.md` is the
entry point (what the suite is, the pages that exist, how a page is read, and the two
maintenance rules). `guides/using/first-request.md` walks sign-in → key → provider →
model → request → confirmation, with the live error bodies for `401` and
`no_provider`. `guides/using/move-an-sdk.md` shows the two changed values in Python
(`openai` 3.46.0) and Node (24), the SSE answer, and the four first-try mistakes.

Rules the whole suite inherits, now enforced by `scripts/check-guides.py` and a CI
`docs` job the deploy waits on:

- every relative link resolves **inside `guides/`** — a reader can open every target;
- no published page links to `inferon-cost-audit.md` or `inferon-step2-materials.md`
  (sales working documents stay internal);
- every published page ends with `_Last verified <date> against <where> (master <sha>)._`;
- a page under `guides/troubleshooting/` carries the five sections in order
  (**Symptom → What it means → Fix → How to confirm it worked → When to stop and ask**);
- mean sentence under 25 words, no sentence over 40.

**Acceptance Criteria**
1. `guides/README.md` links the pages that exist and states how to read one. ✔
2. Guides 1–2 run end to end on the live host; their outputs are quoted from the run. ✔
3. Every command was executed on 2026-11-05 against `master f2165a3`; the footer says so. ✔
4. The checker was proven by deliberately broken pages: a dead link, a link outside
   `guides/`, a link to an internal document, a missing footer, a troubleshooting page
   without its sections, a 41-word sentence — each failed before the fix. ✔
5. CI runs the check and `deploy` needs it. ✔
6. Deployed: `guides/` and `README.md` on the durable host match the repository. ✔
