// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

#[test]
fn a_mark_records_the_time_since_the_previous_one() {
    let mut stages = Stages::new();
    std::thread::sleep(std::time::Duration::from_millis(20));
    stages.mark("auth");
    let auth = stages.get("auth").expect("auth was marked");
    assert!(auth >= 15, "auth took the sleep: {auth} µs");
    assert!(
        stages.get("route").is_none(),
        "unmarked stages have no value"
    );
}

#[test]
fn marks_accumulate_under_their_own_name() {
    let mut stages = Stages::new();
    stages.mark("auth");
    stages.mark("policy");
    stages.mark("route");
    let total: u64 = ["auth", "policy", "route"]
        .iter()
        .filter_map(|name| stages.get(name))
        .sum();
    assert_eq!(total, stages.total_us(), "every microsecond is attributed");
}

#[test]
fn the_summary_names_every_stage() {
    let mut stages = Stages::new();
    stages.mark("auth");
    stages.mark("policy");
    let summary = stages.summary();
    assert!(summary.contains("auth="), "{summary}");
    assert!(summary.contains("policy="), "{summary}");
    assert!(summary.ends_with("µs"), "units are explicit: {summary}");
}

#[test]
fn an_empty_summary_says_so_instead_of_printing_nothing() {
    assert_eq!(Stages::new().summary(), "no stages");
}

#[test]
fn the_total_is_monotonic() {
    let mut stages = Stages::new();
    let first = stages.total_us();
    std::thread::sleep(std::time::Duration::from_millis(5));
    stages.mark("auth");
    assert!(stages.total_us() >= first);
}

#[test]
fn every_pre_dispatch_stage_is_marked_in_the_request_path() {
    // A stage that stops being marked disappears from the debug log silently —
    // the timings would still look plausible, just incomplete. This reads the
    // two files where the marks live (same statement-as-data guard the SQL
    // shape tests use).
    const PROXY: &str = include_str!("../proxy.rs");
    const ROUTE: &str = include_str!("../proxy/route.rs");
    let marked = format!("{PROXY}{ROUTE}");
    for stage in [
        "auth", "body", "policy", "route", "budget", "chain", "prune",
    ] {
        assert!(
            marked.contains(&format!("stages.mark(\"{stage}\")")),
            "stage `{stage}` is no longer marked in the pre-dispatch path"
        );
    }
}
