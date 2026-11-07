// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

fn payload(messages: Value) -> Value {
    serde_json::json!({"messages": messages})
}

/// A unit vector with `hot` in one axis — enough to control the cosine.
fn unit(axis: usize, weight: f32) -> [f32; EMBED_DIM] {
    let mut vector = [0.0; EMBED_DIM];
    vector[axis] = weight;
    vector
}

#[test]
fn the_system_prompt_is_the_task_text() {
    let request = payload(serde_json::json!([
        {"role": "system", "content": "extract the invoice total"},
        {"role": "user", "content": "here is the invoice"}
    ]));
    let (text, source) = task_text(&request).expect("a task");
    assert_eq!(text, "extract the invoice total");
    assert_eq!(source, Source::System);
}

#[test]
fn without_a_system_prompt_the_first_user_turn_is_used() {
    let request = payload(serde_json::json!([
        {"role": "user", "content": "summarise this"},
        {"role": "user", "content": "and this"}
    ]));
    let (text, source) = task_text(&request).expect("a task");
    assert_eq!(text, "summarise this");
    assert_eq!(source, Source::User);
}

#[test]
fn an_empty_system_prompt_falls_through_to_the_user_turn() {
    let request = payload(serde_json::json!([
        {"role": "system", "content": "   "},
        {"role": "user", "content": "summarise this"}
    ]));
    assert_eq!(task_text(&request).map(|(_, source)| source), Some(Source::User));
}

#[test]
fn a_payload_without_turns_has_no_task() {
    assert!(task_text(&payload(serde_json::json!([]))).is_none());
    assert!(task_text(&serde_json::json!({"model": "x"})).is_none());
}

#[test]
fn cosine_is_one_for_the_same_direction_and_zero_for_orthogonal() {
    assert!((cosine(&unit(0, 1.0), &unit(0, 1.0)) - 1.0).abs() < 1e-6);
    assert!(cosine(&unit(0, 1.0), &unit(1, 0.0)).abs() < 1e-6);
    assert_eq!(cosine(&unit(0, 1.0), &[0.0; EMBED_DIM]), 0.0, "a zero vector is never similar");
}

#[test]
fn near_identical_tasks_share_a_cluster() {
    let keys = vec!["a".to_string(), "b".to_string()];
    // 0.99 cosine: a tenant name changed, the task did not.
    let vectors = vec![unit(0, 1.0), unit(0, 0.99)];
    let labels = assign_clusters(&keys, &vectors, CLUSTER_THRESHOLD);
    assert_eq!(labels, vec!["a", "a"], "one cluster, labelled by its representative");
}

#[test]
fn a_different_task_gets_its_own_cluster() {
    let keys = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let vectors = vec![unit(0, 1.0), unit(0, 0.99), unit(1, 1.0)];
    let labels = assign_clusters(&keys, &vectors, CLUSTER_THRESHOLD);
    assert_eq!(labels, vec!["a", "a", "c"]);
}

#[test]
fn clustering_is_order_independent_and_labelled_by_the_lowest_key() {
    // The same set arriving in the other order must produce the same labels:
    // the walk is by key, not by arrival.
    let keys = vec!["b".to_string(), "a".to_string(), "c".to_string()];
    let vectors = vec![unit(0, 0.99), unit(0, 1.0), unit(1, 1.0)];
    let labels = assign_clusters(&keys, &vectors, CLUSTER_THRESHOLD);
    assert_eq!(labels, vec!["a", "a", "c"]);
}

#[test]
fn a_superset_keeps_the_labels_of_the_prompts_it_shares() {
    // Re-clustering as new tasks arrive must not renumber the old clusters.
    let first = assign_clusters(
        &["a".to_string(), "b".to_string()],
        &[unit(0, 1.0), unit(0, 0.99)],
        CLUSTER_THRESHOLD,
    );
    let second = assign_clusters(
        &["a".to_string(), "b".to_string(), "c".to_string()],
        &[unit(0, 1.0), unit(0, 0.99), unit(1, 1.0)],
        CLUSTER_THRESHOLD,
    );
    assert_eq!(&second[..2], &first[..]);
}

#[test]
#[should_panic(expected = "one vector per key")]
fn a_mismatched_input_is_a_programming_error() {
    assign_clusters(&["a".to_string()], &[], CLUSTER_THRESHOLD);
}
