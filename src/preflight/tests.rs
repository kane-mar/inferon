use super::*;

const MODEL: &str = "deepseek-flash";

fn payload(messages: Value) -> Value {
    serde_json::json!({"model": "deepseek-chat", "messages": messages})
}

#[test]
fn a_clean_payload_records_that_it_was_scanned() {
    // A NULL `request_lint` used to mean both "clean" and "never scanned", so
    // nothing on the dashboard could prove the pass ran. A clean scan now
    // records an empty finding list.
    let p = payload(serde_json::json!([
        {"role": "system", "content": "be terse"},
        {"role": "user", "content": "hi"}
    ]));
    let record = inspect(&p, false, MODEL, &Rules::default());
    assert_eq!(record["kinds"], serde_json::json!([]));
    assert_eq!(record["wasted_tokens_est"], 0);
    assert_eq!(record["wasted_usd_est"], "0");
    assert_eq!(record["version"], VERSION);
}

#[test]
fn a_repeated_large_system_prompt_is_flagged_with_an_estimate() {
    let p = payload(serde_json::json!([
        {"role": "system", "content": "x".repeat(9000)},
        {"role": "user", "content": "hi"}
    ]));
    let found = inspect(&p, true, MODEL, &Rules::default());
    let kinds: Vec<&str> = found["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|k| k.as_str())
        .collect();
    assert!(kinds.contains(&"repeated_system_prompt"));
    assert!(kinds.contains(&"boilerplate"));
    assert!(found["wasted_tokens_est"].as_i64().unwrap() > 2000);
}

#[test]
fn an_oversized_schema_is_flagged() {
    let p = payload(serde_json::json!([{"role": "user", "content": "hi"}]));
    let mut p = p;
    p["tools"] = serde_json::json!([{"type": "function", "function": {"parameters": {"x": "y".repeat(9000)}}}]);
    let found = inspect(&p, false, MODEL, &Rules::default());
    assert_eq!(found["kinds"][0], "oversized_schema");
}

#[test]
fn duplicate_turns_are_flagged_and_counted_once_each() {
    let p = payload(serde_json::json!([
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "different"}
    ]));
    let found = inspect(&p, false, MODEL, &Rules::default());
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
    let found = inspect(&p, false, "deepseek-flash", &Rules::default());
    let tokens = found["wasted_tokens_est"].as_i64().unwrap();
    let usd: f64 = found["wasted_usd_est"]
        .as_str()
        .and_then(|usd| usd.parse().ok())
        .expect("a decimal string");
    // deepseek-flash input is $0.22/M: 250 tokens = $0.000055.
    let expected = tokens as f64 * 0.22 / 1_000_000.0;
    assert!((usd - expected).abs() < 1e-9, "{usd} != {expected}");
}

#[test]
fn a_disabled_rule_never_fires() {
    // A tenant can switch rules off; the rest still run.
    let p = payload(serde_json::json!([
        {"role": "user", "content": "same"},
        {"role": "user", "content": "same"},
        {"role": "user", "content": "q"}
    ]));
    let only_schema = Rules {
        duplicate_turns: false,
        ..Rules::default()
    };
    let found = inspect(&p, false, MODEL, &only_schema);
    assert_eq!(
        found["kinds"],
        serde_json::json!([]),
        "duplicate_turns is off"
    );

    let on = inspect(&p, false, MODEL, &Rules::default());
    assert_eq!(on["kinds"][0], "duplicate_turns");
}

#[test]
fn every_rule_off_still_records_a_scan() {
    let p = payload(serde_json::json!([{"role": "user", "content": "hi"}]));
    let none = Rules {
        repeated_system_prompt: false,
        oversized_schema: false,
        boilerplate: false,
        duplicate_turns: false,
    };
    let found = inspect(&p, true, MODEL, &none);
    assert_eq!(found["kinds"], serde_json::json!([]));
    assert_eq!(found["wasted_tokens_est"], 0);
}

#[test]
fn rules_round_trip_through_their_stored_names() {
    let rules = Rules {
        repeated_system_prompt: true,
        oversized_schema: false,
        boilerplate: true,
        duplicate_turns: false,
    };
    assert_eq!(rules.names(), vec!["repeated_system_prompt", "boilerplate"]);
    assert_eq!(Rules::from_names(&rules.names()), rules);
    // Unknown names are ignored, and an empty list means "nothing enabled".
    assert_eq!(
        Rules::from_names(&["nope".to_string(), "boilerplate".to_string()]),
        Rules {
            repeated_system_prompt: false,
            oversized_schema: false,
            boilerplate: true,
            duplicate_turns: false,
        }
    );
}

#[test]
fn a_schema_near_the_budget_reports_its_exact_size() {
    // 9000 characters of schema, 1000 over the budget: the finding prices the
    // excess exactly, as the dashboard has always shown it.
    let mut p = payload(serde_json::json!([{"role": "user", "content": "hi"}]));
    p["tools"] = serde_json::json!([{"type": "function", "function": {"parameters": {"x": "y".repeat(9000)}}}]);
    let found = inspect(&p, false, MODEL, &Rules::default());
    let bytes = super::schema_bytes(&p).0;
    assert_eq!(found["schema_bytes"], bytes);
    assert_eq!(found["schema_bytes_capped"], false);
    assert_eq!(found["wasted_tokens_est"], ((bytes - SCHEMA_BUDGET_BYTES) / 4) as i64);
}

#[test]
fn a_pathological_schema_is_reported_as_capped_not_measured() {
    // ITEM-127: measuring a megabyte-scale schema meant serialising it on every
    // request. Past the reporting cap the finding says so instead of paying for
    // the exact number — the verdict is the same either way.
    let mut p = payload(serde_json::json!([{"role": "user", "content": "hi"}]));
    p["tools"] = serde_json::json!([{"type": "function", "function": {"parameters": {"x": "y".repeat(2_000_000)}}}]);
    let found = inspect(&p, false, MODEL, &Rules::default());
    assert_eq!(found["kinds"][0], "oversized_schema");
    assert_eq!(found["schema_bytes"], SCHEMA_REPORT_CAP_BYTES);
    assert_eq!(found["schema_bytes_capped"], true);
    assert_eq!(
        found["wasted_tokens_est"],
        ((SCHEMA_REPORT_CAP_BYTES - SCHEMA_BUDGET_BYTES) / 4) as i64
    );
}
