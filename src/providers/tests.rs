    use super::*;

    #[test]
    fn hints_show_only_the_tail() {
        assert_eq!(key_hint("sk-1234567890ab12"), "\u{2026}ab12");
        assert_eq!(key_hint("ab"), "\u{2026}ab");
        assert_eq!(key_hint(""), "\u{2026}");
    }

    #[test]
    fn kinds_are_allowlisted() {
        assert!(is_supported_kind("deepseek"));
        assert!(is_supported_kind("openai-compatible"));
        assert!(is_supported_kind("anthropic"));
        assert!(!is_supported_kind("garbage"));
    }

    #[test]
    fn anthropic_is_the_only_non_openai_kind() {
        assert!(is_anthropic("anthropic"));
        assert!(!is_anthropic("openai-compatible"));
        assert_eq!(default_name("anthropic"), "Anthropic");
    }

    #[test]
    fn json_schema_support_is_conservative() {
        // DeepSeek rejects json_schema; the model family is checked too, so a
        // deepseek model behind a compatible gateway is still downgraded.
        assert!(!supports_json_schema("deepseek", "deepseek-chat"));
        assert!(!supports_json_schema("openai-compatible", "deepseek-v4-pro"));
        // OpenAI (and unknown compatible gateways) support it: never downgrade.
        assert!(supports_json_schema("openai", "gpt-4o"));
        assert!(supports_json_schema("openai-compatible", "llama-3.1-70b"));
    }

    #[tokio::test]
    async fn in_memory_store_round_trips() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let id = store
            .insert(
                tenant,
                &NewProvider {
                    kind: "deepseek".to_string(),
                    name: "My DeepSeek".to_string(),
                    base_url: "https://api.deepseek.com/v1".to_string(),
                    key: "sk-mine-ab12".to_string(),
                    metadata: ProviderMetadata {
                        org: Some("org-1".to_string()),
                        headers: [("X-Trace".to_string(), "on".to_string())].into(),
                    },
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("insert");

        let listed = store.list(tenant).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_hint, "\u{2026}ab12");
        assert!(!listed[0].key_hint.contains("sk-mine"));
        assert_eq!(listed[0].metadata.org.as_deref(), Some("org-1"));

        let resolved = store.resolve_for(tenant, None).await.expect("resolve").expect("configured");
        assert_eq!(resolved.base_url, "https://api.deepseek.com/v1");
        assert_eq!(resolved.api_key, "sk-mine-ab12");
        assert_eq!(resolved.metadata.headers["X-Trace"], "on");

        assert!(store.remove(tenant, id).await.expect("remove"));
        assert!(store.resolve_for(tenant, None).await.expect("resolve").is_none());
    }

    #[tokio::test]
    async fn rotation_updates_the_key_without_recreating_the_row() {
        // ITEM-043 AC5: a provider can be rotated in place; the old id survives
        // and the new key resolves on the request path.
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let id = store
            .insert(
                tenant,
                &NewProvider {
                    kind: "openai".to_string(),
                    name: "Mine".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    key: "sk-old-1111".to_string(),
                    metadata: ProviderMetadata::default(),
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("insert");

        let rotated = store
            .update(
                tenant,
                id,
                &ProviderUpdate {
                    name: Some("Renamed".to_string()),
                    base_url: None,
                    key: Some("sk-new-2222".to_string()),
                    metadata: Some(ProviderMetadata {
                        org: Some("org-2".to_string()),
                        headers: BTreeMap::new(),
                    }),
                    spend_limit_cents: Some(2500),
                },
            )
            .await
            .expect("update");
        assert!(rotated);

        let listed = store.list(tenant).await.expect("list");
        assert_eq!(listed.len(), 1, "rotation does not create a new row");
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].name, "Renamed");
        assert_eq!(listed[0].key_hint, "\u{2026}2222");
        assert_eq!(listed[0].metadata.org.as_deref(), Some("org-2"));
        assert_eq!(listed[0].spend_limit_cents, 2500, "ITEM-047 limit is stored");
        let resolved = store.resolve_for(tenant, None).await.expect("resolve").expect("configured");
        assert_eq!(resolved.api_key, "sk-new-2222", "the rotated key is used");

        // Another tenant cannot rotate it.
        assert!(!store
            .update(Uuid::new_v4(), id, &ProviderUpdate { key: Some("sk-thief".to_string()), ..Default::default() })
            .await
            .expect("update"));
    }

    #[tokio::test]
    async fn the_first_provider_is_default_and_set_default_switches() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let a = store
            .insert(tenant, &new("deepseek", "https://a.test/v1", "sk-a"))
            .await
            .unwrap();
        let b = store
            .insert(tenant, &new("openai", "https://b.test/v1", "sk-b"))
            .await
            .unwrap();

        let is_default = |providers: &[Provider], id| {
            providers.iter().find(|p| p.id == id).unwrap().is_default
        };
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, a), "the first provider is the default");
        assert!(!is_default(&listed, b));

        assert!(store.set_default(tenant, b).await.unwrap());
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, b), "set_default switches the default");
        assert!(!is_default(&listed, a));
        // Another tenant cannot set it, and the ordering is default-first.
        assert!(!store.set_default(Uuid::new_v4(), a).await.unwrap());
        assert_eq!(store.list(tenant).await.unwrap()[0].id, b, "default sorts first");

        // Removing the default promotes the oldest remaining provider.
        assert!(store.remove(tenant, b).await.unwrap());
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, a), "a new default is promoted on removal");
    }

    #[tokio::test]
    async fn resolve_for_falls_back_to_the_default_for_unadvertised_models() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let deep = store
            .insert(tenant, &new("deepseek", "https://deep.test/v1", "sk-deep"))
            .await
            .unwrap();
        let catch_all = store
            .insert(tenant, &new("openai-compatible", "https://catch.test/v1", "sk-catch"))
            .await
            .unwrap();
        store
            .record_models(deep, &["deepseek-chat".to_string()])
            .await
            .unwrap();
        store
            .record_models(catch_all, &["my-custom-model".to_string()])
            .await
            .unwrap();
        // Make the catch-all the default.
        assert!(store.set_default(tenant, catch_all).await.unwrap());
        // A model the deepseek provider advertises routes there...
        let owned = store.resolve_for(tenant, Some("deepseek-chat")).await.unwrap().unwrap();
        assert_eq!(owned.id, deep);
        // ...an advertised custom model routes to its provider...
        let free = store.resolve_for(tenant, Some("my-custom-model")).await.unwrap().unwrap();
        assert_eq!(free.id, catch_all);
        // ...and a model nobody advertises falls back to the default.
        let unknown = store
            .resolve_for(tenant, Some("nobody-advertises-this"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unknown.id, catch_all);
    }

    #[tokio::test]
    async fn resolve_for_picks_the_provider_that_advertises_the_model() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let deep = store
            .insert(tenant, &new("deepseek", "https://deep.test/v1", "sk-deep"))
            .await
            .unwrap();
        let open = store
            .insert(tenant, &new("openai-compatible", "https://open.test/v1", "sk-open"))
            .await
            .unwrap();
        store
            .record_models(deep, &["deepseek-chat".to_string()])
            .await
            .unwrap();
        store
            .record_models(open, &["gpt-4o".to_string()])
            .await
            .unwrap();

        let route = store.resolve_for(tenant, Some("gpt-4o")).await.unwrap().unwrap();
        assert_eq!(route.id, open, "gpt-4o is advertised by the second provider");
        let route = store.resolve_for(tenant, Some("deepseek-chat")).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        // An unadvertised model (or no hint) falls back to the default (first).
        let route = store.resolve_for(tenant, Some("mystery")).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        let route = store.resolve_for(tenant, None).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        assert_eq!(
            store.advertised_models(open).await.unwrap(),
            vec!["gpt-4o".to_string()]
        );
    }

    #[tokio::test]
    async fn record_check_is_tenant_scoped() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://x/v1", "sk-x");
        let id = store.list(tenant).await.unwrap()[0].id;
        assert!(store.list(tenant).await.unwrap()[0].last_check_ok.is_none());
        assert!(store.record_check(tenant, id, false).await.unwrap());
        let row = &store.list(tenant).await.unwrap()[0];
        assert_eq!(row.last_check_ok, Some(false));
        assert!(row.last_check_at.is_some());
        // Another tenant cannot record a check on it.
        assert!(!store.record_check(Uuid::new_v4(), id, true).await.unwrap());
    }

    #[tokio::test]
    async fn tenant_scoping_is_enforced() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://x/v1", "sk-x");
        assert!(store.list(other).await.expect("list").is_empty());
        assert!(store.resolve_for(other, None).await.expect("resolve").is_none());
    }

    #[tokio::test]
    async fn resolve_provider_targets_one_tenant_owned_row() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://mine/v1", "sk-mine");
        let id = store.list(tenant).await.expect("list")[0].id;

        let resolved = store.resolve_provider(tenant, id).await.expect("resolve").expect("found");
        assert_eq!(resolved.base_url, "https://mine/v1");
        assert_eq!(resolved.api_key, "sk-mine");
        // Another tenant can never resolve it, even with the id.
        assert!(store.resolve_provider(other, id).await.expect("resolve").is_none());
    }

    #[test]
    fn parse_model_ids_accepts_openai_and_lenient_shapes() {
        assert_eq!(parse_model_ids(&serde_json::json!({"data":[{"id":"a"},{"id":"b"}]})), vec!["a", "b"]);
        assert_eq!(parse_model_ids(&serde_json::json!({"models":[{"name":"m"}]})), vec!["m"]);
        assert_eq!(parse_model_ids(&serde_json::json!(["x", "y"])), vec!["x", "y"]);
        assert_eq!(
            parse_model_ids(&serde_json::json!({"data":[{"id":"b"},{"id":"a"},{"id":"a"}]})),
            vec!["a", "b"],
            "sorted and deduped"
        );
        assert!(parse_model_ids(&serde_json::json!({})).is_empty());
    }

    #[tokio::test]
    async fn fetch_models_reads_the_openai_list_with_auth() {
        let host = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                axum::Json(serde_json::json!({
                    "data": [{"id": "qwen2.5-7b-instruct"}],
                    "auth": auth
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, host).await.unwrap() });

        let provider = ResolvedProvider {
            id: Uuid::nil(),
            kind: "openai-compatible".to_string(),
            base_url: format!("http://{addr}/v1"),
            api_key: "sk-local".to_string(),
            metadata: Default::default(),
        };
        let models = fetch_models(&reqwest::Client::new(), &provider).await.unwrap();
        assert_eq!(models, vec!["qwen2.5-7b-instruct"]);

        // A dead host is an error, not an empty list.
        let dead = ResolvedProvider {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            ..provider
        };
        assert!(fetch_models(&reqwest::Client::new(), &dead).await.is_err());
    }

    #[test]
    fn local_is_a_supported_kind_and_is_marked() {
        assert!(is_supported_kind("local"));
        assert!(is_local("local"));
        assert!(!is_local("openai"), "a cloud provider is not local");
        assert!(!is_local("openai-compatible"));
    }
