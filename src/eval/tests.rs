use super::*;

fn run(id: u8, scored: i64, passed: i64, failed: i64) -> RunScore {
    RunScore {
        run_id: uuid::Uuid::from_u128(id as u128),
        model: "m".to_string(),
        scored,
        passed,
        failed,
    }
}

#[test]
fn normalization_ignores_whitespace_and_case_only() {
    assert_eq!(normalize("  Hello   World \n"), "hello world");
    assert_ne!(normalize("hello world."), normalize("hello world"));
}

#[test]
fn a_reference_decides_exact_match() {
    let matched = score(Some("42."), None, "  42. ", Outcome::Ok);
    assert_eq!(matched.method, Method::Exact);
    assert!(matched.pass);

    let different = score(Some("42"), None, "43", Outcome::Ok);
    assert_eq!(different.method, Method::Exact);
    assert!(!different.pass, "an outcome-ok answer can still be wrong");
}

#[test]
fn a_schema_decides_when_the_request_carried_one() {
    let schema = serde_json::json!({
        "type": "object",
        "required": ["answer"],
        "properties": {"answer": {"type": "string"}}
    });
    assert!(score(None, Some(&schema), r#"{"answer":"42"}"#, Outcome::Ok).pass);
    let invalid = score(None, Some(&schema), r#"{"other":1}"#, Outcome::Ok);
    assert_eq!(invalid.method, Method::Schema);
    assert!(!invalid.pass);
}

#[test]
fn the_schema_wins_over_an_exact_match() {
    // A reference that matches but violates the contract is still a failure: the
    // client would reject it.
    let schema = serde_json::json!({"type": "object"});
    let checked = score(Some("not json"), Some(&schema), "not json", Outcome::Ok);
    assert_eq!(checked.method, Method::Schema);
    assert!(!checked.pass);
}

#[test]
fn without_a_reference_or_schema_only_the_outcome_is_known() {
    assert!(score(None, None, "anything", Outcome::Ok).pass);
    assert_eq!(score(None, None, "anything", Outcome::Ok).method, Method::Outcome);
    assert!(!score(None, None, "", Outcome::Refused).pass);
}

#[test]
fn a_run_below_the_sample_floor_has_no_rate() {
    assert_eq!(run(1, 9, 9, 0).rate(), None);
    assert_eq!(run(1, 10, 7, 0).rate(), Some(0.7));
}

#[test]
fn two_small_runs_are_never_a_verdict() {
    let verdict = compare(&run(1, 9, 9, 0), &run(2, 9, 0, 0));
    assert_eq!(verdict.as_str(), "insufficient");
}

#[test]
fn a_measurably_worse_challenger_regresses() {
    let verdict = compare(&run(1, 100, 95, 0), &run(2, 100, 80, 0));
    assert_eq!(verdict.as_str(), "regressed");
    assert!(matches!(verdict, Verdict::Regressed { baseline, challenger }
        if (baseline - 0.95).abs() < 1e-9 && (challenger - 0.80).abs() < 1e-9));
}

#[test]
fn a_gap_inside_the_margin_is_unchanged() {
    assert_eq!(compare(&run(1, 100, 95, 0), &run(2, 100, 92, 0)).as_str(), "unchanged");
}

#[test]
fn a_clearly_better_challenger_is_an_improvement() {
    assert_eq!(compare(&run(1, 100, 90, 0), &run(2, 100, 98, 0)).as_str(), "improved");
}

#[test]
fn a_partial_challenger_is_insufficient_not_zero() {
    // AC5: a run that lost cases is reported, never silently scored as 0%.
    let verdict = compare(&run(1, 20, 18, 0), &run(2, 20, 0, 20));
    assert_eq!(verdict.as_str(), "insufficient");
    assert!(run(2, 20, 0, 20).is_partial());
}

#[test]
fn the_answer_text_reads_both_surfaces() {
    let openai = serde_json::json!({"choices": [{"message": {"content": "hello"}}]});
    assert_eq!(answer_text(&openai), "hello");
    let anthropic = serde_json::json!({"content": [{"type": "text", "text": "he"},
                                                   {"type": "text", "text": "llo"}]});
    assert_eq!(answer_text(&anthropic), "hello");
    assert_eq!(answer_text(&serde_json::json!({})), "");
}
