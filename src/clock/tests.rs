    use super::*;

    #[test]
    fn now_helpers_agree_and_are_positive() {
        let secs = now_secs();
        assert!(secs > 1_700_000_000, "a plausible wall clock");
        assert_eq!(now_secs_i64(), secs as i64);
        assert!(now_ms().div_euclid(1000).abs_diff(secs) <= 1);
    }
