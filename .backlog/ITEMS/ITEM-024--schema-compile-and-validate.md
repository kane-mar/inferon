## ITEM-024: Schema Guard — Compile + Validate (Slice 4 · was 021b)

**As a** developer with a structured-output contract
**I want** my JSON-schema contract actually enforced on the response
**So that** "guaranteed schema-valid" is more than a slogan

**Acceptance Criteria:**
1. Accumulated guarded content is parsed with `serde_json` and validated against the request's schema compiled with the `jsonschema` crate — content under validation is `choices[0].message.content` (per spec `schema_guard.rs`)
2. Failure reasons are classified per the spec's `fallback_circuit.rs` list: invalid JSON syntax emitted by model / malformed JSON schema contract / schema validation failed / corrupted non-JSON container / guarded-endpoint 5xx or connection crash
3. Unit tests cover known-good and known-bad fixtures for each reason class (TDD); an empty/missing `choices[0].message.content` on success is a defined edge case, not a panic

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021b
- Depends on ITEM-023 (the buffered seam feeding it content)
- Schema source is the request's own `response_format.json_schema.schema` — no new client contract

## Verification record (2026-09-06)

- **AC1 (parse + validate against request schema)**: `guard::extract_content`
  concatenates streamed `choices[0].delta.content` frames in order into the
  accumulated message content; `guard::validate_content` parses it with
  `serde_json` and validates with `jsonschema::validator_for` against the
  schema extracted from the request's own `response_format.json_schema.schema`
  (no new client surface). Slot into the flush gate:
  `check_guarded_sse` = container integrity then content conformance.
- **AC2 (failure classes)**: each spec class maps to a `GuardReason` with a
  stable label — invalid JSON syntax emitted by model -> `InvalidJsonSyntax`;
  request schema won't compile -> `MalformedSchemaContract`; well-formed JSON
  failing the contract -> `SchemaViolation`; corrupted non-JSON container +
  truncated guarded stream (5xx/connection crash) stay `CorruptedContainer` /
  `UpstreamFailure` from ITEM-023's container gate.
- **AC3 (TDD fixtures per class, edge case defined)**: unit fixtures cover
  good+valid, invalid JSON syntax, schema-violating well-formed JSON, malformed
  schema contract, complete stream with no content (defined edge case ->
  `SchemaViolation`, never a panic), no-contract passthrough, and the combined
  container-then-content gate order. Seam tests prove a schema-valid response
  is flushed whole after [DONE] and a schema violation is answered 502
  `guard_rejected`, not forwarded (traces still emitted). 117 tests green.
- Not in this slice: ITEM-025 replaces the rejection's bare 502 with the
  transparent replay to the frontier model.
