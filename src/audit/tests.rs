// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    #[test]
    fn a_share_token_is_prefixed_and_unique() {
        let token = mint_token();

        assert!(token.starts_with(TOKEN_PREFIX));
        assert!(token.len() > TOKEN_PREFIX.len() + 30, "high-entropy body");
        assert_ne!(token, mint_token());
    }

    #[tokio::test]
    async fn a_share_resolves_until_it_is_revoked() {
        let store = InMemoryAuditShareStore::default();
        let tenant = Uuid::new_v4();
        let hash = crate::sha256_hex(&mint_token());
        let id = store.create(tenant, "prospect", &hash).await.unwrap();

        assert_eq!(store.resolve(&hash).await.unwrap(), Some(tenant));
        assert_eq!(store.list(tenant).await.unwrap().len(), 1);
        assert!(store.list(Uuid::new_v4()).await.unwrap().is_empty());

        assert!(store.revoke(tenant, id).await.unwrap());
        assert_eq!(store.resolve(&hash).await.unwrap(), None, "revoked");
        assert!(!store.revoke(tenant, id).await.unwrap(), "already revoked");
    }

    #[tokio::test]
    async fn another_tenant_cannot_revoke_a_share() {
        let store = InMemoryAuditShareStore::default();
        let tenant = Uuid::new_v4();
        let hash = crate::sha256_hex("token");
        let id = store.create(tenant, "", &hash).await.unwrap();

        assert!(!store.revoke(Uuid::new_v4(), id).await.unwrap());
        assert!(store.resolve(&hash).await.unwrap().is_some());
    }

    fn dec(value: &str) -> Decimal {
        Decimal::from_str_exact(value).unwrap()
    }

    #[test]
    fn cache_miss_waste_is_spend_only() {
        let waste = waste_json(vec![(
            "cache_miss".to_string(),
            "deepseek-chat".to_string(),
            String::new(),
            2,
            100,
            0,
            dec("0.01"),
        )]);

        assert_eq!(waste["cache_miss"]["requests"], 2);
        assert_eq!(waste["cache_miss"]["spend_usd"], "0.01");
        assert_eq!(waste["cache_miss"]["savings_est_usd"], "0");
    }

    #[test]
    fn waste_rows_for_the_same_flag_are_summed() {
        let waste = waste_json(vec![
            (
                "cache_miss".to_string(),
                "deepseek-chat".to_string(),
                String::new(),
                2,
                100,
                0,
                dec("0.01"),
            ),
            (
                "cache_miss".to_string(),
                "deepseek-chat".to_string(),
                String::new(),
                1,
                50,
                0,
                dec("0.02"),
            ),
        ]);

        assert_eq!(waste["cache_miss"]["requests"], 3);
        assert_eq!(waste["cache_miss"]["spend_usd"], "0.03");
    }

    #[test]
    fn a_row_without_a_suggested_model_has_no_savings() {
        let waste = waste_json(vec![(
            "over_provisioned".to_string(),
            "gpt-4o".to_string(),
            String::new(),
            1,
            1_000,
            0,
            dec("0.01"),
        )]);

        assert_eq!(waste["over_provisioned"]["savings_est_usd"], "0");
    }

    #[test]
    fn a_priced_substitution_has_savings() {
        let waste = waste_json(vec![(
            "over_provisioned".to_string(),
            "gpt-4o".to_string(),
            "gpt-4o-mini".to_string(),
            1,
            1_000_000,
            0,
            dec("5"),
        )]);

        let savings = waste["over_provisioned"]["savings_est_usd"]
            .as_str()
            .expect("a decimal string")
            .parse::<f64>()
            .unwrap();
        assert!(savings > 0.0, "a cheaper suggested model saves money");
    }
