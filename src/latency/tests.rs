use super::*;

#[test]
fn a_small_sample_is_not_a_verdict() {
    let verdict = verdict(MIN_SAMPLES - 1, Some(200), Some(900));
    assert_eq!(verdict.p50_met, None, "too few samples to say anything");
    assert_eq!(verdict.p95_met, None);
}

#[test]
fn the_verdict_carries_the_payload_it_was_measured_at() {
    // ITEM-127: 300k-token prompts cost the proxy real work before dispatch, so a
    // verdict without its payload size says nothing about the fixed work.
    let verdict = verdict(MIN_SAMPLES, Some(1_200), Some(4_000)).with_payload(Some(310_000));
    assert_eq!(verdict.prompt_tokens_p50, Some(310_000));
    assert_eq!(verdict.p50_met, Some(false), "a big payload still misses the budget");
}

#[test]
fn the_floor_is_enough_to_speak() {
    let verdict = verdict(MIN_SAMPLES, Some(200), Some(900));
    assert_eq!(verdict.p50_met, Some(true));
    assert_eq!(verdict.p95_met, Some(true));
}

#[test]
fn missing_percentiles_withhold_the_verdict_even_with_samples() {
    let verdict = verdict(40, None, None);
    assert_eq!(verdict.p50_met, None);
}

#[test]
fn the_p50_budget_is_the_claim_and_it_is_strict() {
    assert_eq!(
        verdict(40, Some(P50_BUDGET_US - 1), None).p50_met,
        Some(true)
    );
    assert_eq!(
        verdict(40, Some(P50_BUDGET_US), None).p50_met,
        Some(false),
        "exactly 1 ms is not under 1 ms"
    );
    assert_eq!(
        verdict(40, Some(P50_BUDGET_US + 1), None).p50_met,
        Some(false)
    );
}

#[test]
fn the_p95_guard_is_separate_from_the_claim() {
    // A fast p50 with a broken tail still reports honestly: the claim holds, the
    // guard does not.
    let verdict = verdict(40, Some(200), Some(P95_GUARD_US + 1));
    assert_eq!(verdict.p50_met, Some(true));
    assert_eq!(verdict.p95_met, Some(false));
}
