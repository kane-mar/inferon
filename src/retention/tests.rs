// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

#[test]
fn the_purge_statements_null_every_payload_derived_column() {
    // A unit guard for the invariant that a purge leaves nothing behind that
    // could reconstruct the pair or keep counting it as an example.
    for column in [
        "raw_prompt",
        "raw_completion",
        "example_state",
        "example_reason",
        "completion_hash",
    ] {
        assert!(
            NULL_PAYLOADS.contains(column),
            "purge must clear {column}: {NULL_PAYLOADS}"
        );
    }
}
