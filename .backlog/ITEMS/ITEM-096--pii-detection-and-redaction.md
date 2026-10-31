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
---

**Delivered (2026-10-31).** `src/pii.rs` — deterministic scanners (no service, no
model) for email, phone, card (13–19 digits + Luhn), SSN shape (area 000/666/9xx
excluded) and API-key shapes (`sk-`, `AKIA`, `ghp_`/`gho_`/`github_pat_`).
Overlapping candidates are resolved before counting, so a card number is never
also a phone; `redact` writes `<class>` placeholders and is idempotent (a second
pass finds nothing it wrote). `scan_value`/`redact_value` walk a JSON payload,
keys included.

Storage: detection and redaction happen in `build_trace`, before the payload is
stored — so the dataset export and the audit share inherit the redaction with no
second code path. The trace record (`traces.pii`) is `{version, classes, total,
storage, wire}`; no matched value is ever written, logged or traced. Wire
redaction runs after pruning and before dispatch, with a deterministic 10%
holdout (`pii::WIRE_HOLDOUT`) so a tenant can see what redaction costs; the
stored payload is what the model actually saw, so a dataset pair always matches.

**Cost (measured).** 1.53 ms for a 200 KB payload in release (~7.6 µs/KB, so
~30 µs for a typical 4 KB prompt), single linear pass; a unit test fails if the
pass stops being linear. Detection is skipped entirely when the kill switch is
off.

**Limits (stated, not a guarantee).** Shapes only: names, addresses, free-form
identifiers and obfuscated values ("j o h n at example dot com") are invisible;
a clean scan means "no shapes found". `pii_version` is stored with every count so
detector changes are never compared as the same measurement.

**Verified:** 13 unit tests (each class, Luhn, overlaps, idempotence, large
payload, no-value-in-record) + 3 handler tests (counts by default, storage
redaction, wire mode with its holdout, kill switch).
