// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

#[test]
fn the_edge_runtime_is_gone() {
    for path in RETIRED_EDGE_PATHS {
        assert!(
            !std::path::Path::new(path).exists(),
            "{path} is back: retiring the edge node was a decision, not an oversight"
        );
    }
}

#[test]
fn the_cloud_has_no_edge_routes() {
    let lib = include_str!("../lib.rs");
    for route in RETIRED_EDGE_ROUTES {
        assert!(
            !lib.contains(route),
            "{route} is back in the router: the edge profile was retired"
        );
    }
}

#[test]
fn nothing_declares_an_edge_module() {
    let lib = include_str!("../lib.rs");
    for module in ["pub mod edge;", "pub mod edge_runtime;"] {
        assert!(!lib.contains(module), "{module} is back");
    }
}

#[test]
fn the_local_provider_kind_survives_the_retirement() {
    // The reachable-endpoint profile is a different thing and must not be
    // collateral damage: `local` providers still price at zero (ITEM-077).
    let providers = include_str!("../providers.rs");
    assert!(
        providers.contains("\"local\""),
        "the local provider kind is gone"
    );
    assert!(
        crate::cost::is_priced("deepseek-flash"),
        "pricing is intact for hosted models"
    );
}
