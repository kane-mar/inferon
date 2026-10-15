## ITEM-074: Move bulk test modules out of production files

**As a** maintainer
**I want** each production file to be about its own responsibility
**So that** reading a module is not buried under thousands of lines of tests

**Context:** several modules are dominated by their inline `#[cfg(test)] mod
tests` blocks — `lib.rs` is ~310 lines of code but ~2,900 lines of tests, and
`dashboard/mod.rs`, `auth.rs`, `providers.rs` are similar. The tests are valuable
and stay; they should live beside the module, not inside its production file.

**Acceptance Criteria:**
1. Each large file's test module moves to a sibling file referenced by a
   one-line `#[cfg(test)] mod tests;` — e.g. `src/tests.rs` for the crate root,
   `src/dashboard/tests.rs`, `src/auth/tests.rs`, `src/providers/tests.rs`.
2. **No test changes** — the same tests, the same coverage; only their location
   moves. `cargo test` is green with the identical count.
3. Production files shrink to code + the one-line declaration.

**Technical Notes:**
- The crate root's `mod tests;` resolves to `src/tests.rs`; a submodule's
  resolves inside its directory.
- A moved test module usually needs `use crate::...` / `use super::*` adjustments
  (a test file is a sibling module, so its `super` is the parent directory, not
  the module under test — import the module explicitly, e.g. `use crate::proxy::*;`).
- Do it one file at a time with the test count checked between moves.

**Source:** 2026-10-15 codebase review — deferred structural candidate.
