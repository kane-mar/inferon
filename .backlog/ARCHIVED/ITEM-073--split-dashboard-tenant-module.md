## ITEM-073: Split the tenant control-plane module (dashboard/mod.rs)

**As a** maintainer
**I want** the tenant control-plane to be a set of focused modules
**So that** each concern is discoverable and a change touches one file

**Context (2026-10-15 restructure):** `dashboard.rs` was split into
`dashboard/mod.rs` (tenant surface + shared plumbing) and `dashboard/operator.rs`
(operator surface). `dashboard/mod.rs` is still ~2,100 code lines (plus ~1,500
test lines) and mixes: session/scope plumbing, spend queries, provider + model
CRUD, API-key management, member management, routing/config, and edge-node admin.

**Acceptance Criteria:**
1. Shared plumbing stays in `dashboard/mod.rs` — `Caller` + scope/session
   resolution, `json_ok` / `internal` / `db_err` / `unauthorized` / `forbidden`,
   `owner_only`, `Window`, and the pure query helpers.
2. Handlers move to focused submodules re-exported from `mod.rs`, so router paths
   in `build_router` are unchanged: at least `providers` (providers + models),
   `keys`, `members`, `edge_admin`, and `queries` (the tenant spend/quality/
   timeseries endpoints).
3. No behaviour change: Rust tests green, clippy clean, live smoke unchanged.

**Technical Notes:**
- Extract whole top-level items with the column-0 `}` splitter used for
  `proxy.rs` / `dashboard/operator.rs`; make a shared helper `pub(super)` only if
  a sibling (not a child) needs it.
- Keep the operator surface (`dashboard/operator.rs`) as-is — the operator
  endpoints stay in one file.

**Source:** 2026-10-15 codebase review — deferred structural candidate.

## Close-out (2026-10-15)

- `dashboard/mod.rs` keeps the **shared plumbing** (scope/session resolution,
  `json_ok`/`internal`/`db_err`/`unauthorized`/`forbidden`, `owner_only`,
  `Window`) and the tests; the handlers moved to focused submodules, each
  re-exported (`pub use <mod>::*`) so router paths in `build_router` are
  unchanged:
  - `dashboard/providers.rs` — provider + model surface
  - `dashboard/keys.rs` — tenant API-key surface
  - `dashboard/members.rs` — membership surface
  - `dashboard/edge_admin.rs` — edge-node enroll / list / revoke
  - `dashboard/routing.rs` — routing + budget/config
  - `dashboard/queries.rs` — aggregate queries and their read endpoints
- `dashboard/mod.rs`: **3,684 → 1,828 lines**.
- Rust **306** tests, clippy clean. Commit `7556f5f`; CI `38682056834`
  (test+web+deploy) green.
- **Live**: all 16 tenant endpoints (`providers`, `keys`, `routing`, `members`,
  `edge/nodes`, `summary`/`by-*`/`waste`/`timeseries`/`guard`/`models`/`quality`/
  `config`) and all 8 operator endpoints returned **200**.
