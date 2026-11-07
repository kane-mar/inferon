// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

fn kinds(changes: &[Change]) -> Vec<&'static str> {
    changes.iter().map(|change| change.kind.as_str()).collect()
}

#[test]
fn identical_prompts_have_no_changes() {
    let text = "line one\nline two";
    let changes = diff(text, text);
    assert!(changes.iter().all(|change| change.kind == Kind::Same));
    assert_eq!(changes.len(), 2);
    assert_eq!(kinds(&changes), vec!["same", "same"]);
}

#[test]
fn an_inserted_line_is_one_addition() {
    let changes = diff("a\nb", "a\nnew\nb");
    assert_eq!(kinds(&changes), vec!["same", "added", "same"]);
    assert_eq!(changes[1].text, "new");
}

#[test]
fn a_removed_line_is_one_removal() {
    let changes = diff("a\ngone\nb", "a\nb");
    assert_eq!(kinds(&changes), vec!["same", "removed", "same"]);
    assert_eq!(changes[1].text, "gone");
}

#[test]
fn a_replaced_line_shows_both_sides() {
    let changes = diff("be terse", "be very terse");
    assert_eq!(kinds(&changes), vec!["removed", "added"]);
    assert_eq!(changes[0].text, "be terse");
    assert_eq!(changes[1].text, "be very terse");
}

#[test]
fn the_diff_is_symmetric_in_its_counts() {
    let before = "one\ntwo\nthree";
    let after = "one\nthree\nfour";
    let forward = diff(before, after);
    let backward = diff(after, before);
    let count = |changes: &[Change], kind: Kind| {
        changes.iter().filter(|change| change.kind == kind).count()
    };
    assert_eq!(count(&forward, Kind::Added), count(&backward, Kind::Removed));
    assert_eq!(count(&forward, Kind::Removed), count(&backward, Kind::Added));
}

#[test]
fn a_prompt_past_the_bound_is_reported_as_a_whole_replacement() {
    let long = "x\n".repeat(MAX_DIFF_LINES + 1);
    let changes = diff(&long, "short");
    assert_eq!(changes.last().unwrap().kind, Kind::Added);
    // Honest about not comparing: everything before is "removed".
    assert!(changes.iter().filter(|c| c.kind == Kind::Removed).count() > MAX_DIFF_LINES);
}
