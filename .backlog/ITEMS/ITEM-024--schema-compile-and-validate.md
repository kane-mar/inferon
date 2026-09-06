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
