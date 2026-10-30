## ITEM-096: PII detection and optional redaction

**As a** tenant with regulated data
**I want** to know (and optionally remove) PII before it is stored or forwarded
**So that** the dataset surface does not become a liability

**Claim (2026-10-30 brief, landscape).** Portkey provides "PII masking". Inferon
stores raw prompts and completions verbatim and has no detection.

**Current state (2026-10-30).** No detection, no masking, no policy. Raw payloads are
stored by default (see ITEM-088).

**Gap.** No PII handling at all.

**Acceptance Criteria**
1. Pattern-based detection (email, phone, card-number with Luhn, SSN-like, API-key
   shapes) runs on the trace path by default and records **counts by class** — never
   the matched value.
2. Per-tenant **redaction mode** (opt-in) replaces matches in the stored payload and,
   optionally, in the forwarded request; the mode is traced and visible in the
   dashboard.
3. Redaction is irreversible in storage and honoured by the dataset export
   (ITEM-087) and the audit share.
4. No external service and no ML model: deterministic Rust patterns, benchmarked to
   a stated cost, with a kill switch.
5. False positives/negatives are documented; the detector's limits are stated
   (it is not a compliance guarantee).
6. Tests: each class, overlapping matches, redaction idempotence, export honouring
   the mode.

**Technical Notes**
- Detection on the stored payload is enough for v1; forwarding redaction changes the
  prompt and therefore the answer, so it is a separate opt-in with the same holdout
  discipline as ITEM-091.
- Do not log a match value anywhere (only the class and a count).

**Ranking note.** Rank **12** of 22 · V3/E2.5 — a gate for regulated datasets; parity, but its value rises with 087.