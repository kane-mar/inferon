// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

fn request(messages: Value) -> Value {
    serde_json::json!({
        "model": "deepseek-flash",
        "messages": messages,
        "tools": [{"type": "function", "function": {"name": "lookup"}}],
    })
}

fn on() -> PrunePolicy {
    PrunePolicy {
        enabled: true,
        ..PrunePolicy::default()
    }
}

#[test]
fn pruning_is_off_by_default() {
    let payload = request(serde_json::json!([
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "question"}
    ]));
    assert!(!PrunePolicy::default().enabled);
    assert!(rewrite(&payload, &PrunePolicy::default()).is_none());
}

#[test]
fn an_exact_duplicate_turn_is_dropped_and_the_question_is_untouched() {
    let payload = request(serde_json::json!([
        {"role": "system", "content": "be terse"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "assistant", "content": "ok"},
        {"role": "user", "content": "now the real question"}
    ]));
    let rewrite = rewrite(&payload, &on()).expect("a rewrite");
    assert_eq!(rewrite.removals, vec![Removal::DuplicateTurn]);
    let kept = rewrite.payload["messages"].as_array().unwrap();
    assert_eq!(kept.len(), 4, "one duplicate removed");
    assert_eq!(kept[0]["role"], "system", "the system prompt is never touched");
    assert_eq!(
        kept.last().unwrap()["content"], "now the real question",
        "the last turn is the question"
    );
}

#[test]
fn the_last_turn_is_never_removed_even_when_it_repeats_an_earlier_one() {
    // The first turn stays: it is the canonical copy, and the last turn is the
    // question. Removing the earlier one would only lose context.
    let payload = request(serde_json::json!([
        {"role": "user", "content": "again"},
        {"role": "assistant", "content": "ok"},
        {"role": "user", "content": "again"}
    ]));
    assert!(rewrite(&payload, &on()).is_none());

    // With a third copy in the middle, the middle one goes.
    let thrice = request(serde_json::json!([
        {"role": "user", "content": "again"},
        {"role": "user", "content": "again"},
        {"role": "user", "content": "again"}
    ]));
    let rewrite = rewrite(&thrice, &on()).expect("a rewrite");
    let kept = rewrite.payload["messages"].as_array().unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept.last().unwrap()["content"], "again");
}

#[test]
fn tool_definitions_are_never_rewritten() {
    let payload = request(serde_json::json!([
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "q"}
    ]));
    let rewrite = rewrite(&payload, &on()).expect("a rewrite");
    assert_eq!(
        rewrite.payload["tools"], payload["tools"],
        "the contract is passed through untouched"
    );
}

#[test]
fn a_token_budget_drops_the_oldest_turns_and_keeps_the_question() {
    let long = "x".repeat(400); // 100 estimated tokens
    let payload = request(serde_json::json!([
        {"role": "system", "content": "sys"},
        {"role": "user", "content": long},
        {"role": "assistant", "content": long},
        {"role": "user", "content": "the question"}
    ]));
    let policy = PrunePolicy {
        enabled: true,
        drop_duplicate_turns: true,
        max_prompt_tokens: Some(40),
        holdout_percent: 0,
    };
    let rewrite = rewrite(&payload, &policy).expect("a rewrite");
    let kept = rewrite.payload["messages"].as_array().unwrap();
    assert_eq!(kept.len(), 2, "both long turns went");
    assert_eq!(kept[0]["role"], "system");
    assert_eq!(kept[1]["content"], "the question");
    assert!(
        rewrite.removals.iter().all(|removal| *removal == Removal::StaleTurn),
        "the budget, not the dedupe, removed them"
    );
}

#[test]
fn a_payload_with_nothing_to_remove_is_forwarded_as_is() {
    let payload = request(serde_json::json!([
        {"role": "user", "content": "a"},
        {"role": "assistant", "content": "b"},
        {"role": "user", "content": "c"}
    ]));
    assert!(rewrite(&payload, &on()).is_none());
    assert!(rewrite(&serde_json::json!({"model": "x"}), &on()).is_none());
}

#[test]
fn the_event_records_kinds_bytes_tokens_usd_and_the_original_hash() {
    let payload = request(serde_json::json!([
        {"role": "user", "content": "y".repeat(400)},
        {"role": "user", "content": "y".repeat(400)},
        {"role": "user", "content": "q"}
    ]));
    let rewrite = rewrite(&payload, &on()).expect("a rewrite");
    let event = rewrite.event("deepseek-flash", 7);
    assert_eq!(event["policy_version"], 7);
    assert_eq!(event["kinds"]["duplicate_turn"], 1);
    assert_eq!(event["turns_removed"], 1);
    assert!(event["bytes_saved"].as_u64().unwrap() > 0);
    assert!(event["tokens_saved_est"].as_i64().unwrap() > 0);
    assert!(event["usd_saved_est"].as_str().unwrap().parse::<f64>().unwrap() > 0.0);
    assert_eq!(event["original_prompt_hash"], rewrite.original_hash);
}

#[test]
fn the_holdout_is_deterministic_and_bounded() {
    let policy = PrunePolicy {
        enabled: true,
        holdout_percent: 10,
        ..PrunePolicy::default()
    };
    let seed = "prompt-hash";
    assert_eq!(in_holdout(seed, &policy), in_holdout(seed, &policy));
    let held = (0..1000)
        .filter(|index| in_holdout(&format!("{seed}-{index}"), &policy))
        .count();
    assert!(
        (50..=150).contains(&held),
        "≈10% of 1000 seeds, got {held}"
    );
    assert!(!in_holdout(seed, &PrunePolicy { holdout_percent: 0, ..on() }));
}

#[test]
fn a_misconfigured_holdout_can_never_take_everything() {
    let policy = PrunePolicy {
        enabled: true,
        holdout_percent: 200,
        ..PrunePolicy::default()
    };
    let held = (0..100).filter(|index| in_holdout(&format!("s{index}"), &policy)).count();
    assert!(held <= 50, "clamped to 50%: got {held}");
}

#[test]
fn a_small_sample_is_never_a_verdict() {
    assert!(!should_disable(0, 5, 5, 5), "5 requests per arm proves nothing");
    assert!(!should_disable(18, 20, 19, 20), "a 5-point gap is inside the margin");
}

#[test]
fn pruning_that_hurts_is_disabled() {
    // 60% ok pruned vs 95% ok held out: a 35-point drop, well past the margin.
    assert!(should_disable(60, 100, 95, 100));
    // 92% vs 95%: within the margin, keep saving.
    assert!(!should_disable(92, 100, 95, 100));
    // Better pruned than held out: never a reason to stop.
    assert!(!should_disable(99, 100, 90, 100));
}
