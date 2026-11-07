// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    #[tokio::test]
    async fn settings_are_provider_scoped_and_defaulting_clears_the_provider() {
        let provider = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryModelStore::default();

        store.set_enabled(provider, "deepseek-flash", false).await.unwrap();
        assert!(store.is_disabled(Some(provider), "deepseek-flash").await.unwrap());
        assert!(!store.is_disabled(Some(other), "deepseek-flash").await.unwrap(), "other provider untouched");
        assert!(!store.is_disabled(None, "deepseek-flash").await.unwrap(), "shared key unrestricted");
        // An unconfigured model is not disabled (unrestricted).
        assert!(!store.is_disabled(Some(provider), "deepseek-v4-pro").await.unwrap());

        store.set_default(provider, "deepseek-chat").await.unwrap();
        store.set_default(provider, "deepseek-v4-pro").await.unwrap();
        let settings = store.settings(provider).await.unwrap();
        let defaults: Vec<_> = settings.iter().filter(|s| s.is_default).collect();
        assert_eq!(defaults.len(), 1, "one default per provider");
        assert_eq!(defaults[0].model_id, "deepseek-v4-pro");
    }
