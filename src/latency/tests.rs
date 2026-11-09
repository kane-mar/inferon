// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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

#[test]
fn a_prompt_lands_in_the_band_its_size_belongs_to() {
    // ITEM-136: the boundaries are the claim's own basis, so the small band is
    // exactly the traffic the 1 ms claim was written about.
    assert_eq!(band_of(Some(35)), "small");
    assert_eq!(band_of(Some(SMALL_PROMPT_TOKENS)), "small", "the boundary is inclusive");
    assert_eq!(band_of(Some(SMALL_PROMPT_TOKENS + 1)), "medium");
    assert_eq!(band_of(Some(50_000)), "medium");
    assert_eq!(band_of(Some(50_001)), "large");
    assert_eq!(band_of(Some(150_000)), "large");
    assert_eq!(band_of(Some(150_001)), "huge");
    assert_eq!(band_of(Some(285_000)), "huge");
}

#[test]
fn a_trace_without_token_counts_is_never_placed_in_a_band() {
    // Guessing would put a request in a band it was not measured in, which is
    // exactly the mistake this report exists to stop.
    assert_eq!(band_of(None), "unknown");
}

#[test]
fn every_band_has_a_contiguous_bound() {
    // The query builds its predicates from these, so a gap or an overlap would
    // silently drop or double-count traffic.
    let mut previous: Option<i64> = None;
    for (index, band) in BANDS.iter().enumerate() {
        let (lower, upper) = band_bounds(index);
        match previous {
            None => assert_eq!(lower, 0, "the first band starts at zero"),
            Some(prev) => assert_eq!(lower, prev + 1, "bands meet without a gap: {band:?}"),
        }
        assert!(upper.is_none_or(|u| u >= lower), "a band is never inverted");
        previous = upper;
    }
    assert_eq!(previous, None, "the last band is open-ended");
}

#[test]
fn a_band_that_slows_at_the_same_payload_is_drifting() {
    // ITEM-137: the shape of a real regression — same work, more time.
    assert_eq!(drift(Some(1_000), Some(1_100), Some(200_000), Some(205_000)), Some(Drift::Calm));
    assert_eq!(drift(Some(1_000), Some(1_300), Some(200_000), Some(205_000)), Some(Drift::Watch));
    assert_eq!(drift(Some(1_000), Some(1_600), Some(200_000), Some(205_000)), Some(Drift::Regression));
}

#[test]
fn a_band_that_slows_because_the_prompts_grew_is_not_drifting() {
    // The payload moved 40%: these are different requests, and calling the
    // second one a regression is the misreport the bands exist to prevent.
    assert_eq!(drift(Some(1_000), Some(9_000), Some(100_000), Some(140_000)), Some(Drift::Moved));
    // Shrinking counts too — a smaller prompt that is slower *is* worth a look,
    // but it is not comparable to the baseline either.
    assert_eq!(drift(Some(1_000), Some(9_000), Some(300_000), Some(100_000)), Some(Drift::Moved));
}

#[test]
fn the_boundary_of_comparability_is_inclusive() {
    // Exactly 20% is still comparable, a hair over is not.
    assert_eq!(drift(Some(1_000), Some(1_000), Some(100_000), Some(120_000)), Some(Drift::Calm));
    assert_eq!(drift(Some(1_000), Some(1_000), Some(100_000), Some(121_000)), Some(Drift::Moved));
}

#[test]
fn missing_numbers_are_never_a_verdict() {
    // No baseline, no bucket value, no token counts: say nothing.
    assert_eq!(drift(None, Some(1_000), Some(100), Some(100)), None);
    assert_eq!(drift(Some(1_000), None, Some(100), Some(100)), None);
    assert_eq!(drift(Some(0), Some(1_000), Some(100), Some(100)), None);
    assert_eq!(drift(Some(1_000), Some(1_500), None, Some(100)), Some(Drift::Moved));
    assert_eq!(drift(Some(1_000), Some(1_500), Some(100), None), Some(Drift::Moved));
}
