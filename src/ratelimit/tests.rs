use super::*;

fn limits(rpm: i64, tpm: i64) -> Limits {
    Limits {
        requests_per_minute: rpm,
        tokens_per_minute: tpm,
    }
}

#[test]
fn no_limits_means_unlimited() {
    assert!(Limits::default().is_unlimited());
    assert_eq!(
        decide(Limits::default(), Usage { requests: 9_999, tokens: 9_999_999 }, 5),
        Decision::Allowed(Usage { requests: 9_999, tokens: 9_999_999 })
    );
}

#[test]
fn the_request_boundary_is_limit_minus_one_limit_limit_plus_one() {
    let limits = limits(2, 0);
    assert!(matches!(
        decide(limits, Usage { requests: 1, tokens: 0 }, 10),
        Decision::Allowed(_)
    ));
    assert!(matches!(
        decide(limits, Usage { requests: 2, tokens: 0 }, 10),
        Decision::Allowed(_)
    ));
    assert_eq!(
        decide(limits, Usage { requests: 3, tokens: 0 }, 10),
        Decision::Limited {
            axis: Axis::Requests,
            limit: 2,
            retry_after_secs: 10
        }
    );
}

#[test]
fn tokens_are_a_second_axis_with_its_own_limit() {
    let limits = limits(10, 100);
    assert!(matches!(
        decide(limits, Usage { requests: 1, tokens: 100 }, 30),
        Decision::Allowed(_)
    ));
    assert_eq!(
        decide(limits, Usage { requests: 1, tokens: 101 }, 30),
        Decision::Limited {
            axis: Axis::Tokens,
            limit: 100,
            retry_after_secs: 30
        }
    );
}

#[test]
fn velocity_wins_over_tokens_when_both_are_over() {
    // Requests is the axis a client can reason about first.
    assert!(matches!(
        decide(limits(1, 1), Usage { requests: 2, tokens: 2 }, 5),
        Decision::Limited { axis: Axis::Requests, .. }
    ));
}

#[test]
fn the_retry_after_is_the_next_minute_boundary() {
    assert_eq!(retry_after_secs(0), 60);
    assert_eq!(retry_after_secs(59), 1);
    assert_eq!(retry_after_secs(61), 59);
}

#[test]
fn the_prompt_estimate_is_chars_over_four() {
    assert_eq!(estimate_tokens(0), 0);
    assert_eq!(estimate_tokens(3), 0);
    assert_eq!(estimate_tokens(4), 1);
    assert_eq!(estimate_tokens(4_000), 1_000);
}

#[test]
fn a_zero_limit_on_one_axis_leaves_the_other_enforced() {
    let limits = limits(0, 50);
    assert!(matches!(
        decide(limits, Usage { requests: 1_000, tokens: 50 }, 1),
        Decision::Allowed(_)
    ));
    assert!(matches!(
        decide(limits, Usage { requests: 1_000, tokens: 51 }, 1),
        Decision::Limited { axis: Axis::Tokens, .. }
    ));
}
