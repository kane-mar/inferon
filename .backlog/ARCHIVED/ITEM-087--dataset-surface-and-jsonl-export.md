## ITEM-087: Dataset surface and JSONL export

**As a** tenant owner
**I want** to see the dataset my traffic has produced and download it as JSONL
**So that** the proxy's output is an asset I can fine-tune with, not just an invoice

**Claim (2026-10-30 brief).** "Instead of just giving a developer an invoice,
Inferon says: 'You've spent $400 on GPT-4o for this specific extraction prompt. We
have captured 2,000 verified examples. Click here to download the fine-tuning
JSONL.'"

**Current state (2026-10-30).** No export of any kind. `audit::report` (public
share) is a read-only cost report; `by-prompt` aggregates cache hit rate, not
examples.

**Gap.** No dataset list, no export, no provenance, no guard against exporting
data the tenant did not consent to keep.

**Acceptance Criteria**
1. A "Datasets" surface lists prompt clusters ranked by qualified examples, each
   with: spend, verified count, models, first/last seen, and estimated frontier
   saving if the cluster ran on the cheap/distilled model.
2. Owner-only export of one cluster (or all) as **JSONL instruction pairs** in the
   OpenAI fine-tune shape (`{"messages":[system?, user, assistant]}`), including
   `model` and `trace_id` as provenance on each line.
3. Export streams (no full materialisation), is bounded by an explicit limit, and
   is **audited**: who exported what, when, row count, byte count.
4. Export refuses to run when the tenant's capture switch is off or the cluster
   contains no qualified examples (typed 4xx with the reason).
5. The dashboard shows the same numbers as the API (one read model, two surfaces).
6. A JSONL line round-trips: re-ingesting the exported pair through a test harness
   reproduces the same outcome classification.

**Technical Notes**
- Build on ITEM-086's aggregate; export reads raw payloads only for qualified rows.
- Deterministic ordering (prompt hash, then trace time) so two exports of the same
  data are byte-identical.
- PII review of exported content is ITEM-096's job; this item must not pretend to
  scrub.

**Depends on** ITEM-085, ITEM-086, ITEM-088 (consent gate).
**Ranking note.** Rank **4** of 22 · V5/E2.5 — the demonstrable moment of the pitch; needs 086 (counts) and 088 (consent).