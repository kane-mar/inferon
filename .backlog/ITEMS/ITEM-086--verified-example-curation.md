## ITEM-086: Verified-example curation and per-prompt counts

**As a** tenant with a high-value extraction prompt
**I want** Inferon to count the examples it has verified for that prompt
**So that** "you have captured 2,000 verified examples" is a fact, not a hope

**Claim (2026-10-30 brief).** "We have captured 2,000 verified examples" for a
specific prompt, out of production traffic. Today there is no notion of a verified
example, no per-prompt count, and no dedupe.

**Current state (2026-10-30).** The ingredients exist: `outcome`
(ok/empty/refused/error/truncated), thumbs (`trace_quality`), the schema guard, and
`system_prompt_hash` grouping. The guard fired 0 times in the last 7 days, so
`outcome = ok` plus "full completion captured" is the only signal at scale today.

**Gap.** No qualification rule, no dedupe, no count or spend-per-prompt read model.

**Acceptance Criteria**
1. An example **qualifies** when: `outcome = ok`, a complete (uncapped) completion
   was captured, it is not truncated, and the client did not thumb it down. When
   the request carried a `json_schema`, the guard must have validated or repaired
   the response (a guard `fallback` counts as repaired; `fallback_failed` does not).
2. Requests with no schema qualify on the signals above; the criteria are returned
   with the counts so the difference is explicit.
3. **Dedupe** at this stage is exact: one example per
   `(system_prompt_hash, normalized completion)`; the count and the spend are
   reported per `(prompt_hash, model)`.
4. A read-only endpoint returns, per prompt cluster: requests, spend, qualified
   count, excluded count by reason, models, and the last-seen time. It is backed by
   an indexed aggregate, not a scan of raw payloads per request.
5. Thumbs-down and tenant-denylisted prompts are excluded and the reason is
   visible — never silently dropped.

**Technical Notes**
- Store the qualification decision on the trace at write time
  (e.g. `example_state` + `example_reason`) so the aggregate is a `COUNT(*)`, not
  a re-derivation over JSONB.
- "spend on this prompt" reuses the existing spend aggregate; no new cost path.
- Near-duplicate dedupe is deliberately out of scope — that is ITEM-089.

**Depends on** ITEM-085 (completions must exist to qualify).
**Ranking note.** Rank **2** of 22 · V5/E2 — turns captured bytes into a countable asset and defines "verified"; 087 depends on it.