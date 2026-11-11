## ITEM-147: Refactor review - unused, duplicated and over-complex code

**Requested** 2026-11-10: "review the code and structure ... look for unused code,
unnecessary code, overly complex code or code that's not intentional. Do not
degrade the performance ... use clean code and tdd."

**Method.** Tools first, opinion second: `knip` (web exports and files), `clippy`
with dead-code linting, a function-length scan across `src/`, a duplicate-block scan
across the Rust tree, and `check-guides`/`i18n-check` for copy. Then a test-first
refactor of what the evidence pointed at, touching only code with a test or a live
check behind it.

**Found and fixed**

1. **Two duration formatters that disagreed.** `reportFormat.ms` printed
   `1,500 ms`, `dashboardMetrics.formatMs` printed `1.50s`, and the chart axis had
   a third copy. The overview and the runtime page therefore showed the same metric
   two different ways. `libs/format.ts` is now the only home (`duration`, `micros`,
   `percent`, `integer`), written test-first: 10 tests, then both old formatters
   became delegations.
2. **The specs were duplicated too.** `dashboardMetrics.test.ts` and
   `reportFormat.test.ts` each asserted their own copy of the percent and duration
   rules. That is how the two formats drifted. The rules are specified once, in
   `format.test.ts`; the duplicates are gone.
3. **Names that lied.** `ms()` started returning seconds once unified. Call sites
   now say `duration()`, `micros()`, `percent()`.
4. **Unused:** `hasDocs` (found by knip), an `integer` re-export, and
   `chrono_free_now` once its only caller stopped needing it. knip is clean.
5. **Over-complex by testability, not length.** The operator's policy handler held
   its rules inline: bounds, known objective, what an unnamed field keeps, and the
   evidence merge. They are pure now (`policy::OperatorChange::apply`, returning
   `PolicyError`), covered by 6 tests that need no request. The handler went from
   ~90 lines to ~40 and has one job: gate, load, publish.
6. **Duplicated table atoms.** Two pages declared the same `HEAD`/`RIGHT` pairs;
   they are exported once from `reportFormat`.

**Evidence it is safe.** 174 web tests (up from 169), 605 Rust tests (up from 599),
tsc, eslint, knip, i18n-check and the guides check clean. Live: the runtime page
renders 13 durations in seconds and 3 in ms, with **0** of the old mixed
`1,234 ms` form; the refactored validator still refuses `0.95` with a 400 and
publishes `0.3` with the loop's prompt observation preserved in evidence.

**Deliberately not touched**

- **The long SQL-and-JSON handlers** (`query_summary`, `bands`, `dispatch_chain`,
  `translate::*`): their length is a query and a literal, and there is no unit test
  protecting the seam. Splitting them would add indirection and risk on the hot
  path for no measurable gain.
- **`src/tests.rs` at 5,027 lines.** Splitting is mechanical but the diff would bury
  every other change, and nothing behaves differently for it.
- **`pub fn new(pool)` boilerplate** repeated across stores: a macro would be worse
  than the repetition.
- **Performance:** no hot-path code moved. The formatter change is server-side
  string building on the page render, not in the proxy; the proxy diff is the policy
  validator, which runs once per operator request.

**Source:** PO request, 2026-11-10.
