use super::*;

fn candidates(weights: &[f64]) -> Vec<Candidate> {
    weights
        .iter()
        .enumerate()
        .map(|(slot, weight)| Candidate { slot, weight: *weight })
        .collect()
}

#[test]
fn a_single_provider_is_chosen_unchanged() {
    let choice = choose(&candidates(&[0.4]), 42, Strategy::Weighted).unwrap();
    assert_eq!(choice.slot, 0);
    assert_eq!(choice.reason, Reason::Only, "no distribution to do");
}

#[test]
fn nothing_eligible_means_no_choice() {
    assert_eq!(choose(&[], 0, Strategy::Weighted), None);
    assert_eq!(choose(&candidates(&[0.0, f64::NAN]), 0, Strategy::Weighted), None);
}

#[test]
fn a_zeroed_provider_is_skipped_even_if_it_is_the_only_rich_one() {
    // The policy demoted provider 0 to zero: every request goes to 1.
    let choice = choose(&candidates(&[0.0, 0.5]), 1, Strategy::Weighted).unwrap();
    assert_eq!(choice.slot, 1);
    assert_eq!(choice.reason, Reason::Only);
}

#[test]
fn round_robin_alternates_deterministically() {
    let candidates = candidates(&[1.0, 1.0, 1.0]);
    let slots: Vec<usize> = (0..6)
        .map(|seed| choose(&candidates, seed, Strategy::RoundRobin).unwrap().slot)
        .collect();
    assert_eq!(slots, vec![0, 1, 2, 0, 1, 2]);
}

#[test]
fn weighted_selection_follows_the_weights() {
    // 3:1 in thousandths: three of every four seeds go to slot 0.
    let candidates = candidates(&[0.75, 0.25]);
    let mut counts = [0usize; 2];
    for seed in 0..1_000u64 {
        counts[choose(&candidates, seed, Strategy::Weighted).unwrap().slot] += 1;
    }
    assert_eq!(counts, [750, 250]);
}

#[test]
fn a_degraded_provider_gets_less_not_nothing() {
    // Health weights 0.6 / 0.3: both serve, the healthier one twice as often.
    let candidates = candidates(&[0.6, 0.3]);
    let mut counts = [0usize; 2];
    for seed in 0..900u64 {
        counts[choose(&candidates, seed, Strategy::Weighted).unwrap().slot] += 1;
    }
    assert_eq!(counts, [600, 300]);
}

#[test]
fn the_same_seed_and_weights_always_choose_the_same_provider() {
    let candidates = candidates(&[0.5, 0.3, 0.2]);
    for seed in 0..100u64 {
        let first = choose(&candidates, seed, Strategy::Weighted);
        let again = choose(&candidates, seed, Strategy::Weighted);
        assert_eq!(first, again);
    }
}

#[test]
fn share_reports_the_expected_distribution() {
    let candidates = candidates(&[0.6, 0.3]);
    assert!((share(&candidates, 0) - 2.0 / 3.0).abs() < 1e-9);
    assert!((share(&candidates, 1) - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(share(&candidates, 9), 0.0, "unknown slot");
}

#[test]
fn consecutive_counters_spread_across_the_weight_space() {
    // Without the mix, seeds 1..=20 all land in the first 1000-wide bucket.
    let candidates = candidates(&[1.0, 1.0]);
    let mut counts = [0usize; 2];
    for counter in 1..=20u64 {
        counts[choose(&candidates, mix(counter), Strategy::Weighted).unwrap().slot] += 1;
    }
    assert!(counts[0] >= 5 && counts[1] >= 5, "both buckets get traffic: {counts:?}");
}

#[test]
fn the_mix_is_deterministic() {
    assert_eq!(mix(7), mix(7));
    assert_ne!(mix(7), mix(8));
}
