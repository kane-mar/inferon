use super::*;

const MODEL: &str = "deepseek-flash";

fn payload(messages: Value) -> Value {
    serde_json::json!({"model": "deepseek-chat", "messages": messages})
}

#[test]
fn a_clean_payload_has_no_findings() {
    let p = payload(serde_json::json!([
        {"role": "system", "content": "be terse"},
        {"role": "user", "content": "hi"}
    ]));
    assert!(inspect(&p, false, MODEL).is_none());
}

#[test]
fn a_repeated_large_system_prompt_is_flagged_with_an_estimate() {
    let p = payload(serde_json::json!([
        {"role": "system", "content": "x".repeat(9000)},
        {"role": "user", "content": "hi"}
    ]));
    let found = inspect(&p, true, MODEL).expect("findings");
    let kinds: Vec<&str> = found["kinds"].as_array().unwrap().iter().filter_map(|k| k.as_str()).collect();
    assert!(kinds.contains(&"repeated_system_prompt"));
    assert!(kinds.contains(&"boilerplate"));
    assert!(found["wasted_tokens_est"].as_i64().unwrap() > 2000);
}

#[test]
fn an_oversized_schema_is_flagged() {
    let p = payload(serde_json::json!([{"role": "user", "content": "hi"}]));
    let mut p = p;
    p["tools"] = serde_json::json!([{"type": "function", "function": {"parameters": {"x": "y".repeat(9000)}}}]);
    let found = inspect(&p, false, MODEL).expect("findings");
    assert_eq!(found["kinds"][0], "oversized_schema");
}

#[test]
fn duplicate_turns_are_flagged_and_counted_once_each() {
    let p = payload(serde_json::json!([
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "different"}
    ]));
    let found = inspect(&p, false, MODEL).expect("findings");
    assert_eq!(found["kinds"][0], "duplicate_turns");
    assert_eq!(found["wasted_tokens_est"], 1);
}

#[test]
fn the_waste_is_priced_at_the_request_model() {
    // 9,000 chars > the 8,000 boilerplate budget => 250 wasted tokens.
    let p = payload(serde_json::json!([
        {"role": "system", "content": "x".repeat(9000)},
        {"role": "user", "content": "hi"}
    ]));
    let found = inspect(&p, false, "deepseek-flash").expect("findings");
    let tokens = found["wasted_tokens_est"].as_i64().unwrap();
    let usd: f64 = found["wasted_usd_est"]
        .as_str()
        .and_then(|usd| usd.parse().ok())
        .expect("a decimal string");
    // deepseek-flash input is $0.22/M: 250 tokens = $0.000055.
    let expected = tokens as f64 * 0.22 / 1_000_000.0;
    assert!((usd - expected).abs() < 1e-9, "{usd} != {expected}");
}
