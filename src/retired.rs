// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Things the product deliberately no longer has, and why.
//!
//! A retired surface is not dead code to be found later: it is a decision. These
//! guards fail if one of them comes back without the decision being revisited —
//! the same class of protection the SQL shape tests give a statement.
//!
//! **Edge node (`--edge`), retired 2026-11-01.** It ran the proxy next to a
//! customer's own model and synced config from the cloud, but the cloud could
//! only keep records: prompts could not leave the node, so datasets, eval and
//! adaptation were impossible there, and the node's budget/limits were a stale
//! snapshot of *other* traffic. The full package (ITEM-083, `--profile
//! self-hosted`) does everything the node did and more, inside the customer's
//! own network. Keeping a second data plane meant maintaining every feature
//! twice, and the node's config contract had already drifted (9 of 17 fields).
//!
//! What remains is the profile customers actually want: a **reachable
//! endpoint** — an on-prem model registered as a `local` provider that the
//! cloud proxy calls (ITEM-051/077). That is not the edge.

/// Paths whose absence is part of the retirement.
pub const RETIRED_EDGE_PATHS: [&str; 2] = ["src/edge.rs", "src/edge_runtime.rs"];

/// Routes the retired profile owned.
pub const RETIRED_EDGE_ROUTES: [&str; 2] = ["/v1/edge/traces", "/v1/edge/config"];

#[cfg(test)]
mod tests;
