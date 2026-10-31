## ITEM-097: Prompt registry and versioning

**As a** tenant
**I want** to see how a prompt changed over time and which version produced which
outcomes
**So that** prompt iteration is measurable, not folklore

**Claim (2026-10-30 brief, landscape).** Langfuse offers "prompt-version tracking".
Inferon identifies prompts by an exact content hash and groups by it; it has no
notion of a prompt identity with versions.

**Current state (2026-10-30).** `system_prompt_hash` (content hash) + `by-prompt`
aggregates (requests, cost, cache hit rate, outcomes). A prompt edit becomes a new,
unrelated hash with no link to its predecessor.

**Gap.** No identity, no version history, no diff.

**Acceptance Criteria**
1. A tenant can **name** a prompt (hash → label) and see its versions ordered by
   first-seen, with first/last seen, request counts, spend and outcome mix per
   version.
2. Content is retrievable per version for an owner (the stored prompt payload) with
   a diff against the previous version.
3. Identity remains content-derived: unmanaged prompts keep working and can be named
   retroactively.
4. Version labels are additive metadata, not a rewrite of history; deleting a label
   does not delete traces.
5. The dataset surface can scope counts/exports to a version or a named prompt.
6. Read-only in v1 — no prompt editing or deployment from the registry.

**Technical Notes**
- Store the label on a small `prompt_labels` table keyed by
  `(tenant_id, system_prompt_hash)`; versions are a view over traces ordered by
  first-seen.
- A diff needs the raw prompt, so it obeys ITEM-088's capture switch.

**Ranking note.** Rank **15** of 22 · V2/E2 — prompt tooling — observability parity.
---

**Delivered (2026-10-31).** `prompt_labels (tenant_id, prompt_name,
system_prompt_hash)` — additive by design: identity stays the content hash, so
unmanaged prompts keep working and naming is retroactive. Versions are a view
over `traces` ordered by first-seen, never a rewrite of history.

`GET /api/prompts` returns every name with its versions (first/last seen,
requests, spend, ok rate, thumbs down); a label on a hash no traffic produced is
listed with zero counts rather than hidden. `POST /api/prompts/labels` (owner)
names a hash and is idempotent; `DELETE /api/prompts/labels/:name/:hash` drops
the label only. `GET /api/prompts/diff` returns an LCS line diff of two
versions' captured system prompts — owner-only and refused when the tenant does
not capture payloads (ITEM-088).

**Scoping**: `GET /api/datasets?prompt=<name>` and the export both restrict to
the hashes labelled with that name (AC5).

**Verified**: 6 unit tests on the diff (identical, insert, remove, replace,
symmetric counts, the size bound). The DB surface is exercised live after the
next deploy.
