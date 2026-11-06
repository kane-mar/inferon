## ITEM-128: The output checker verifies only streamed answers, and only hard-coded model names

**As a** workspace owner who turned the checker on
**I want** every structured answer checked against its schema
**So that** "the checker is on" means the same thing for a buffered call and a streamed one

**Found 2026-11-06 while writing the ITEM-118 configuration guides.** Two coverage gaps,
both reproduced on the live gateway with a mock provider:

1. **Non-streaming answers were never checked.** A `json_schema` request without
   `stream: true` went straight through `forward_buffered` — no validation, no
   `guard_event` on the trace, and a schema-violating answer was returned to the client
   with `200`. Streamed requests were checked. The most common structured-extraction
   shape (a plain call, no streaming) was the unchecked one.
2. **"Cheap tier" was a hard-coded list of model names** (`gpt-4o-mini`, `deepseek-chat`,
   `deepseek-flash`, `deepseek-v4-flash`). A pair the workspace defined itself
   (`my-frontier → my-small`) and any model it hosts (`kind: local`) were never checked,
   even though those are exactly the models the router downshifts to.

**Fix.** `guard::check_guarded_json` validates `choices[0].message.content` for a
buffered answer; `AnswerShape` carries the differences (check, usage, outcome,
content-type) through the guard, the flush and the replay; `is_cheap_tier` now takes the
workspace's tier pairs and whether the model is local.

**Acceptance Criteria**
1. A buffered `json_schema` request served by a cheap tier whose answer violates the
   schema is replayed on the frontier (or the chain's `guard` step), and the client
   receives the valid answer — never the broken one. ✔
2. The trace records `guard_event` (`fallback`, reason, guarded model) for that
   request. ✔
3. The cheap side of a workspace pair, and any local model, count as cheap; a frontier
   model still does not. ✔
4. Existing stream behaviour is unchanged (the stream tests still pass byte for byte). ✔
5. Deployed and verified on the live host: a non-streaming schema request with a
   violating answer returns the replay's answer and a `guard_event`.

**Source:** ITEM-118 evidence run, 2026-11-06.
