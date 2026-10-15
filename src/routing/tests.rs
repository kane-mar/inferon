    use super::*;

    #[test]
    fn only_retryable_failures_trigger_a_fallback() {
        assert_eq!(fallback_reason(429), Some(FallbackReason::RateLimited));
        assert_eq!(fallback_reason(500), Some(FallbackReason::ServerError));
        assert_eq!(fallback_reason(503), Some(FallbackReason::ServerError));
        // A request problem is surfaced as-is, never retried on another model.
        assert_eq!(fallback_reason(400), None);
        assert_eq!(fallback_reason(401), None);
        assert_eq!(fallback_reason(404), None);
        assert_eq!(fallback_reason(200), None);
    }

    #[tokio::test]
    async fn chains_round_trip_and_are_tenant_scoped() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryRoutingStore::default();
        assert!(store.fallbacks(tenant).await.unwrap().is_empty(), "no chain by default");

        store
            .set_fallbacks(tenant, &["gpt-4o-mini".to_string(), "deepseek-flash".to_string()])
            .await
            .unwrap();
        assert_eq!(
            store.fallbacks(tenant).await.unwrap(),
            vec!["gpt-4o-mini", "deepseek-flash"],
            "order is preserved"
        );
        assert!(store.fallbacks(other).await.unwrap().is_empty(), "other tenant untouched");
    }
