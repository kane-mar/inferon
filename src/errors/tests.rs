// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    #[test]
    fn upstream_status_maps_to_a_class() {
        assert_eq!(ErrorClass::from_upstream_status(429), ErrorClass::Upstream429);
        assert_eq!(ErrorClass::from_upstream_status(500), ErrorClass::Upstream5xx);
        assert_eq!(ErrorClass::from_upstream_status(503), ErrorClass::Upstream5xx);
        assert_eq!(ErrorClass::from_upstream_status(400), ErrorClass::Client4xx);
        assert_eq!(ErrorClass::from_upstream_status(401), ErrorClass::Client4xx);
        assert_eq!(ErrorClass::from_upstream_status(404), ErrorClass::Client4xx);
    }

    #[test]
    fn class_strings_round_trip() {
        for class in [
            ErrorClass::Upstream429,
            ErrorClass::Upstream5xx,
            ErrorClass::Client4xx,
            ErrorClass::Inferon4xx,
            ErrorClass::Inferon5xx,
            ErrorClass::Transport,
            ErrorClass::Timeout,
            ErrorClass::Guard,
            ErrorClass::ClientAbort,
        ] {
            assert_eq!(ErrorClass::from_db(Some(class.as_str())), Some(class));
        }
        assert_eq!(ErrorClass::from_db(None), None);
        assert_eq!(ErrorClass::from_db(Some("garbage")), None);
    }
