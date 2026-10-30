    use super::*;
    use crate::TenantStore;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// Router state with an InMemory tenant store; `db` is a lazy pool that is
    /// never actually queried by the auth/index tests below.
    const SEEDED_TENANT: &str = "22222222-2222-2222-2222-222222222222";

    fn seeded_tenant_store() -> std::sync::Arc<crate::InMemoryTenantStore> {
        let mut tenants = std::collections::HashMap::new();
        let hash = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(b"sk-dashboard-dev"))
        };
        tenants.insert(
            hash,
            crate::Tenant {
                id: uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(),
                upstream_openai_key: Some("sk-master".to_string()),
                api_key_id: None,
            },
        );
        std::sync::Arc::new(crate::InMemoryTenantStore::new(tenants))
    }

    fn state_with(
        accounts: Option<std::sync::Arc<crate::auth::Accounts>>,
        tenant_store: std::sync::Arc<dyn crate::TenantStore>,
    ) -> std::sync::Arc<crate::AppState> {
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        std::sync::Arc::new(crate::AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/".to_string(),
            tenant_store,
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(crate::budget::InMemoryBudgetStore::default()),
            hotpath: std::sync::Arc::new(crate::hotpath::HotPath::new()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: Some("op-secret".to_string()),
            accounts,
            db: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(300))
                .connect_lazy("postgres://inferon:inferon@127.0.0.1:1/inferon")
                .unwrap(),
        })
    }

    fn dashboard_state() -> std::sync::Arc<crate::AppState> {
        dashboard_state_full(None)
    }

    /// Same seeded state, with the given accounts (ITEM-036 session-scope
    /// tests inject an OAuth-configured `accounts` here).
    fn dashboard_state_full(
        accounts: Option<std::sync::Arc<crate::auth::Accounts>>,
    ) -> std::sync::Arc<crate::AppState> {
        state_with(accounts, seeded_tenant_store())
    }

    fn get(uri: &str, key: Option<&str>) -> axum::http::Request<Body> {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(key) = key {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    fn post(uri: &str, key: Option<&str>, body: &str) -> axum::http::Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(key) = key {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn operator_key_endpoints_gate_then_manage_keys() {
        // ITEM-034 AC2/AC3: the operator key API is gated like the other
        // operator views and drives the mint/list/revoke lifecycle over the
        // in-memory store (same semantics as the Postgres store).
        let state = dashboard_state();
        let router = crate::build_router(state);
        const TID: &str = "22222222-2222-2222-2222-222222222222";
        let keys_uri = "/v1/dashboard/api/operator/keys";
        let list_uri = format!("{keys_uri}?tenant_id={TID}");

        // Scope gates (no key -> 401, tenant key -> 403).
        let missing_list = router.clone().oneshot(get(&list_uri, None)).await.unwrap();
        assert_eq!(missing_list.status(), axum::http::StatusCode::UNAUTHORIZED, "list no key");
        let tenant_list = router
            .clone()
            .oneshot(get(&list_uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(tenant_list.status(), axum::http::StatusCode::FORBIDDEN, "list tenant key");
        let tenant_create = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("sk-dashboard-dev"),
                &format!(r#"{{"tenant_id":"{TID}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(tenant_create.status(), axum::http::StatusCode::FORBIDDEN, "create tenant key");

        // Mint: operator key -> raw key shown once, key id returned.
        let create = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("op-secret"),
                &format!(r#"{{"tenant_id":"{TID}","label":"ci"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(create.status(), axum::http::StatusCode::OK);
        let created: Value = serde_json::from_slice(
            &axum::body::to_bytes(create.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let raw_key = created["key"].as_str().expect("raw key shown once").to_string();
        let key_id = created["key_id"].as_str().expect("key id").to_string();
        assert!(raw_key.starts_with(crate::CLIENT_KEY_PREFIX));

        // List shows metadata — never key material.
        let listed = router
            .clone()
            .oneshot(get(&list_uri, Some("op-secret")))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK);
        let list_json: Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let list_text = list_json.to_string();
        assert!(list_text.contains("ci"), "label present");
        assert!(
            !list_text.contains(&raw_key) && !list_text.contains("sk-inferon-"),
            "list never returns key material"
        );

        // Revoke -> listed as revoked; the raw key stops authenticating is
        // pinned by the proxy seam test (lib.rs).
        let revoke = router
            .clone()
            .oneshot(post(
                &format!("/v1/dashboard/api/operator/keys/{key_id}/revoke"),
                Some("op-secret"),
                "",
            ))
            .await
            .unwrap();
        let revoke_status = revoke.status();
        let revoke_body = axum::body::to_bytes(revoke.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(
            revoke_status,
            axum::http::StatusCode::OK,
            "revoke body: {} (key_id {key_id})",
            String::from_utf8_lossy(&revoke_body)
        );
        let listed = router
            .clone()
            .oneshot(get(&list_uri, Some("op-secret")))
            .await
            .unwrap();
        let list_json: Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let ci = list_json["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["label"] == "ci")
            .expect("ci key listed");
        assert_eq!(ci["revoked"], true);

        // Unknown tenant mint -> 404; unknown key revoke -> 404.
        let unknown_tenant = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("op-secret"),
                r#"{"tenant_id":"99999999-9999-9999-9999-999999999999","label":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(unknown_tenant.status(), axum::http::StatusCode::NOT_FOUND);
        let unknown_key = router
            .clone()
            .oneshot(post(
                "/v1/dashboard/api/operator/keys/99999999-9999-9999-9999-999999999999/revoke",
                Some("op-secret"),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(unknown_key.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dashboard_apis_require_a_valid_tenant_key() {
        let state = dashboard_state();
        let router = crate::build_router(state);
        for uri in [
            "/v1/dashboard/api/summary",
            "/v1/dashboard/api/by-model",
            "/v1/dashboard/api/by-prompt",
            "/v1/dashboard/api/by-provider",
            "/v1/dashboard/api/by-key",
            "/v1/dashboard/api/waste",
        ] {
            let missing = router
                .clone()
                .oneshot(get(uri, None))
                .await
                .unwrap();
            assert_eq!(missing.status(), axum::http::StatusCode::UNAUTHORIZED, "{uri} without key");
            let unknown = router
                .clone()
                .oneshot(get(uri, Some("sk-wrong")))
                .await
                .unwrap();
            assert_eq!(unknown.status(), axum::http::StatusCode::UNAUTHORIZED, "{uri} with unknown key");
        }
    }

    #[tokio::test]
    async fn owner_sets_a_key_spend_limit_and_members_cannot() {
        // ITEM-047 AC2: per-key limits are owner-managed and reflected in the
        // key list; a member gets 403.
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let key_id = json["keys"][0]["id"].as_str().expect("seeded key id").to_string();

        let set = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/limit"),
                &token,
                r#"{"spend_limit_cents":2500}"#,
            ))
            .await
            .unwrap();
        assert_eq!(set.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(set).await["spend_limit_cents"], 2500);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(json["keys"][0]["spend_limit_cents"], 2500);

        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/limit"),
                &member_token,
                r#"{"spend_limit_cents":1}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn operator_scope_is_enforced_before_any_query() {
        let state = dashboard_state();
        let router = crate::build_router(state.clone());
        let uri = "/v1/dashboard/api/operator/tenants?days=7";

        // Endpoint gates (no DB touched): missing -> 401, tenant key -> 403.
        let missing = router
            .clone()
            .oneshot(get(uri, None))
            .await
            .unwrap();
        assert_eq!(missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let tenant_key = router
            .clone()
            .oneshot(get(uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(tenant_key.status(), axum::http::StatusCode::FORBIDDEN);

        // Tenant endpoints reject the operator credential.
        let op_on_tenant_view = router
            .clone()
            .oneshot(get("/v1/dashboard/api/summary", Some("op-secret")))
            .await
            .unwrap();
        assert_eq!(op_on_tenant_view.status(), axum::http::StatusCode::FORBIDDEN);

        // ITEM-026: the guard views share the same scope gates (no DB touched
        // before the gate): tenant guard view needs a tenant key, operator
        // guard view needs the operator key.
        let tenant_guard = "/v1/dashboard/api/guard?days=7";
        let guard_missing = router.clone().oneshot(get(tenant_guard, None)).await.unwrap();
        assert_eq!(guard_missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let guard_op = router.clone().oneshot(get(tenant_guard, Some("op-secret"))).await.unwrap();
        assert_eq!(guard_op.status(), axum::http::StatusCode::FORBIDDEN);

        let operator_guard = "/v1/dashboard/api/operator/guard?days=7";
        let op_guard_missing = router.clone().oneshot(get(operator_guard, None)).await.unwrap();
        assert_eq!(op_guard_missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let op_guard_tenant = router
            .clone()
            .oneshot(get(operator_guard, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(op_guard_tenant.status(), axum::http::StatusCode::FORBIDDEN);

        // ITEM-020 AC2: the operator alerts view has the same gate.
        let alerts_uri = "/v1/dashboard/api/operator/alerts?days=7";
        let alerts_no_key = router.clone().oneshot(get(alerts_uri, None)).await.unwrap();
        assert_eq!(alerts_no_key.status(), axum::http::StatusCode::UNAUTHORIZED);
        let alerts_tenant = router
            .clone()
            .oneshot(get(alerts_uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(alerts_tenant.status(), axum::http::StatusCode::FORBIDDEN);

        // Scope resolution itself (pure, no DB): operator key -> Operator,
        // tenant key -> Tenant, unknown -> None.
        fn bearer(key: &str) -> axum::http::HeaderMap {
            let mut h = axum::http::HeaderMap::new();
            h.insert("authorization", format!("Bearer {key}").parse().unwrap());
            h
        }
        assert_eq!(
            resolve_scope(&state, &bearer("op-secret")).await.unwrap(),
            Caller { tenant: None, is_operator: true }
        );
        let tenant_key = resolve_scope(&state, &bearer("sk-dashboard-dev")).await.unwrap();
        assert!(tenant_key.tenant.is_some() && !tenant_key.is_operator);
        assert!(resolve_scope(&state, &bearer("sk-nope")).await.is_err());

        // Operator access is disabled when no operator key is configured.
        let mut disabled = dashboard_state();
        std::sync::Arc::get_mut(&mut disabled)
            .unwrap()
            .operator_key = None;
        assert!(resolve_scope(&disabled, &bearer("op-secret")).await.is_err());
    }

    #[test]
    fn window_defaults_and_clamps() {
        let w = |days, limit, offset| Window { days, limit, offset };
        assert_eq!(w(None, None, None).days(), 7);
        assert_eq!(w(Some(3), None, None).days(), 3);
        assert_eq!(w(Some(9999), None, None).days(), 365);
        assert_eq!(w(Some(0), None, None).days(), 1);
        assert_eq!(w(None, Some(500), None).limit(), 100);
        assert_eq!(w(None, Some(3), None).limit(), 3);
        // `offset` shifts the window back for period comparison.
        assert_eq!(w(None, None, None).offset(), 0);
        assert_eq!(w(None, None, Some(30)).offset(), 30);
        assert_eq!(w(None, None, Some(-5)).offset(), 0);
        assert_eq!(w(None, None, Some(99_999)).offset(), 3650);
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn cookie_headers(token: &str) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("inferon_session={token}").parse().unwrap(),
        );
        headers
    }

    /// State where user 42 owns tenant 2222… (the seeded tenant) and holds a
    /// valid session token; returns (state, token, tenant_id).
    async fn signed_in_state() -> (std::sync::Arc<crate::AppState>, String, uuid::Uuid) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(42, "octocat", false).await.expect("ok");
        let tenant_id = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        users.add_membership(tenant_id, user.id, "owner").await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token, tenant_id)
    }

    #[tokio::test]
    async fn session_cookie_grants_the_owners_tenant_scope_without_a_key() {
        let (state, token, tenant_id) = signed_in_state().await;
        let scope = resolve_scope(&state, &cookie_headers(&token)).await.expect("resolved");
        assert_eq!(scope, Caller { tenant: Some(tenant_id), is_operator: false });
    }

    #[tokio::test]
    async fn missing_or_forged_session_stays_unauthorized() {
        // No accounts configured: the session path is inert (pre-036 behavior).
        let state = dashboard_state();
        let err = resolve_scope(&state, &axum::http::HeaderMap::new()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);

        // Forged cookie (wrong secret) with accounts configured.
        let (state, _, _) = signed_in_state().await;
        let forged = crate::session::issue_session(
            uuid::Uuid::new_v4(),
            "not-the-secret",
            now_secs(),
            3600,
        );
        let err = resolve_scope(&state, &cookie_headers(&forged)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_memberless_signed_in_user_stays_unauthorized() {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(7, "nobody", false).await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let err = resolve_scope(&state, &cookie_headers(&token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_keys_and_operator_credentials_win_over_the_session() {
        // The user's session points at tenant A, but the bearer key belongs to
        // the seeded tenant 2222… and the operator key is op-secret — the
        // bearer paths must resolve first (SDKs stay key-authed, additive).
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(9, "owner", false).await.expect("ok");
        users
            .add_membership(
                uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
                user.id,
                "owner",
            )
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let mut headers = cookie_headers(&token);
        headers.insert("authorization", "Bearer sk-dashboard-dev".parse().unwrap());
        let scope = resolve_scope(&state, &headers).await.expect("resolved");
        assert_eq!(
            scope.tenant.map(|id| id.to_string()).as_deref(),
            Some("22222222-2222-2222-2222-222222222222"),
            "tenant key wins over the session"
        );
        assert!(!scope.is_operator);
        let mut headers = cookie_headers(&token);
        headers.insert("authorization", "Bearer op-secret".parse().unwrap());
        let scope = resolve_scope(&state, &headers).await.expect("resolved");
        assert!(
            scope.is_operator && scope.tenant.is_none(),
            "operator key wins over the session"
        );
    }

    // --- ITEM-037 AC1/AC4: operator sessions + isolation ---

    async fn operator_session_state() -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(42, "octocat", true).await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    #[tokio::test]
    async fn an_operator_session_scopes_to_operator_without_a_membership() {
        let (state, token) = operator_session_state().await;
        let scope = resolve_scope(&state, &cookie_headers(&token)).await.expect("resolved");
        assert_eq!(
            scope,
            Caller { tenant: None, is_operator: true },
            "operator session -> platform scope"
        );
    }

    #[tokio::test]
    async fn an_operator_who_owns_a_tenant_reaches_both_scopes() {
        // Operator status and tenant membership are orthogonal: a platform
        // operator who also owns a tenant gets the tenant view AND the
        // platform view (the login-then-403 regression).
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(42, "both", true).await.expect("ok");
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        users.add_membership(tenant_id, user.id, "owner").await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let headers = cookie_headers(&token);

        assert_eq!(
            tenant_or(&state, &headers).await.expect("tenant scope"),
            tenant_id,
            "an operator who owns a tenant still reaches tenant routes"
        );
        assert!(
            operator_or(&state, &headers).await.is_ok(),
            "an operator who owns a tenant still reaches operator routes"
        );
    }

    #[tokio::test]
    async fn operator_and_tenant_sessions_stay_isolated_from_each_others_routes() {
        // An operator session is rejected by tenant-scoped gates (403).
        let (state, op_token) = operator_session_state().await;
        let err = tenant_or(&state, &cookie_headers(&op_token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN, "operator on tenant route");

        // A tenant-member session is rejected by operator gates (403).
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let member = users.upsert_github_user(7, "bob", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::new_v4(), member.id, "member")
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let member_token = crate::session::issue_session(
            member.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let err = operator_or(&state, &cookie_headers(&member_token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN, "member on operator route");
    }

    #[tokio::test]
    async fn session_scoped_request_reaches_the_dashboard_handler() {
        // A session cookie alone must pass the route gate (the lazy pool makes
        // the SQL fail with 500, which proves the scope was granted — a 401
        // would mean the session was ignored).
        let (state, token, _) = signed_in_state().await;
        let router = crate::build_router(state);
        let request = Request::builder()
            .method("GET")
            .uri("/v1/dashboard/api/summary?days=7")
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_ne!(
            response.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "session is not ignored at the route seam"
        );
        assert_ne!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-038: tenant key management (session-scoped, owner/member) ---

    /// A session for a user with `role` on the seeded tenant 2222… over the
    /// seeded tenant store; returns (state, token).
    async fn tenant_role_session(role: &str) -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(11, "t11", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(), user.id, role)
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    fn request_with_cookie(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::empty())
            .unwrap()
    }

    fn post_with_cookie(uri: &str, token: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[test]
    fn rate_is_none_without_a_denominator() {
        assert_eq!(super::rate(1, 2), Some(0.5));
        assert_eq!(super::rate(0, 0), None);
    }

    #[test]
    fn wow_is_none_without_a_baseline() {
        assert_eq!(super::wow(15, 10), Some(0.5));
        assert_eq!(super::wow(5, 10), Some(-0.5));
        assert_eq!(super::wow(10, 0), None);
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn owner_session_lists_creates_and_revokes_tenant_keys() {
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK, "owner lists keys");
        let json: Value = body_json(listed).await;
        assert_eq!(json["role"], "owner");
        assert_eq!(json["keys"].as_array().unwrap().len(), 1, "the seeded dev key");

        let created = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/keys",
                &token,
                r#"{"label":"ci"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), axum::http::StatusCode::OK);
        let created_json: Value = body_json(created).await;
        let raw = created_json["key"].as_str().expect("raw key shown once").to_string();
        let key_id = created_json["key_id"].as_str().expect("key id").to_string();
        assert!(raw.starts_with("sk-inferon-"));

        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(revoke.status(), axum::http::StatusCode::OK, "owner revokes");
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let ci = json["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["label"] == "ci")
            .expect("ci key listed");
        assert_eq!(ci["revoked"], true, "revocation shows immediately");
    }

    #[tokio::test]
    async fn member_can_create_keys_but_revoking_is_403() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let created = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/keys",
                &token,
                r#"{"label":"member-key"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), axum::http::StatusCode::OK, "member may create");
        let json: Value = body_json(created).await;
        let key_id = json["key_id"].as_str().expect("key id").to_string();
        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(
            revoke.status(),
            axum::http::StatusCode::FORBIDDEN,
            "members cannot revoke (AC2)"
        );
    }

    #[tokio::test]
    async fn keys_api_requires_a_human_session() {
        let state = dashboard_state();
        let router = crate::build_router(state);
        let none = router.clone().oneshot(get("/v1/dashboard/api/keys", None)).await.unwrap();
        assert_eq!(none.status(), axum::http::StatusCode::UNAUTHORIZED);
        let bearer = router
            .clone()
            .oneshot(get("/v1/dashboard/api/keys", Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(bearer.status(), axum::http::StatusCode::UNAUTHORIZED, "bare key is not a human");
    }

    #[tokio::test]
    async fn tenant_revoke_cannot_kill_another_tenants_key() {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(11, "t11", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(), user.id, "owner")
            .await
            .expect("ok");
        // The store also holds a second tenant with its own key.
        let store = seeded_tenant_store();
        let other = store.insert_tenant(Some("other")).await.expect("ok");
        let other_key = store.insert_api_key(other, "other-key", "hash-x").await.expect("ok");
        let state = state_with(
            Some(crate::auth::test_helpers::dummy_accounts(users)),
            store,
        );
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let router = crate::build_router(state);
        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{other_key}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(
            revoke.status(),
            axum::http::StatusCode::NOT_FOUND,
            "another tenant's key id is unknown here"
        );
    }

    // --- ITEM-046: routing & budget controls (session-scoped, owner mutates) ---

    #[tokio::test]
    async fn owner_reads_and_updates_the_tenant_controls() {
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);

        // Unconfigured tenant: router + guard on by default, no budget cap.
        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        assert_eq!(read.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(read).await;
        assert_eq!(json["budget_limit_cents"], 0);
        assert_eq!(json["current_spend_cents"], 0);
        assert_eq!(json["guard_enabled"], true);
        assert_eq!(json["router_enabled"], true);
        assert_eq!(json["router_threshold"], 0.35);
        assert_eq!(json["role"], "owner");

        // Owner writes the controls; they read back unchanged.
        let updated = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/config",
                &token,
                r#"{"budget_limit_cents":2500,"cheap_fallback_model":"gpt-4o-mini","guard_enabled":false,"router_enabled":false,"router_threshold":0.8}"#,
            ))
            .await
            .unwrap();
        assert_eq!(updated.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(updated).await["updated"], true);

        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        let json: Value = body_json(read).await;
        assert_eq!(json["budget_limit_cents"], 2500);
        assert_eq!(json["cheap_fallback_model"], "gpt-4o-mini");
        assert_eq!(json["guard_enabled"], false);
        assert_eq!(json["router_enabled"], false);
        assert_eq!(json["router_threshold"], 0.8);
    }

    #[tokio::test]
    async fn member_cannot_update_the_tenant_controls() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        assert_eq!(read.status(), axum::http::StatusCode::OK, "members may view");
        let write = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/config",
                &token,
                r#"{"budget_limit_cents":1,"cheap_fallback_model":null,"guard_enabled":true,"router_enabled":true,"router_threshold":0.35}"#,
            ))
            .await
            .unwrap();
        assert_eq!(write.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-043: provider rotation/metadata (session-scoped, owner mutates) ---

    #[test]
    fn base_url_normalization_trims_and_requires_http() {
        assert_eq!(
            normalize_base_url(" https://api.test/v1/ ").unwrap(),
            "https://api.test/v1"
        );
        assert_eq!(
            normalize_base_url("http://localhost:8080").unwrap(),
            "http://localhost:8080"
        );
        assert!(normalize_base_url("ftp://api.test").is_err());
        assert!(normalize_base_url("api.test").is_err());
    }

    #[test]
    fn upstream_base_url_strips_the_chat_completions_suffix() {
        assert_eq!(
            upstream_base_url("https://api.deepseek.com/v1/chat/completions"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(upstream_base_url("https://api.test/v1/"), "https://api.test/v1");
    }

    #[tokio::test]
    async fn the_providers_response_never_exposes_a_platform_key() {
        // The platform's shared upstream connection is Inferon-private: it is
        // not surfaced on the tenant Providers page/API.
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);
        let listed = router
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert!(json.get("platform").is_none(), "no platform connection exposed");
        assert!(!json.to_string().contains("sk-master"), "never the raw platform key");
    }

    #[tokio::test]
    async fn owner_can_probe_an_unsaved_provider_and_members_cannot() {
        // A mock host exposing GET /v1/models.
        let host = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(json!({"data":[{"id":"m1"},{"id":"m2"}]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, host).await.unwrap() });

        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);
        let body = format!(r#"{{"base_url":"http://{addr}/v1","key":"sk-x"}}"#);
        let probed = router
            .clone()
            .oneshot(post_with_cookie("/v1/dashboard/api/providers/test", &token, &body))
            .await
            .unwrap();
        assert_eq!(probed.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(probed).await;
        assert_eq!(json["ok"], true);
        let ids: Vec<&str> = json["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["m1", "m2"]);

        // A member cannot probe.
        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers/test",
                &member_token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn owner_toggles_a_providers_model_and_members_cannot() {
        // Models come from the host (`/models`), cached per provider; selection
        // is owner-only and only for advertised models.
        let (state, token) = tenant_role_session("owner").await;
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        let provider_id = crate::providers::ProviderStore::insert(
            &*state.provider_store,
            tenant_id,
            &crate::providers::NewProvider {
                kind: "openai-compatible".to_string(),
                name: "My host".to_string(),
                base_url: "https://my-host.test/v1".to_string(),
                key: "sk-host-ab12".to_string(),
                metadata: Default::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .unwrap();
        // What the host advertises (as a `/models` fetch would record).
        crate::providers::ProviderStore::record_models(
            &*state.provider_store,
            provider_id,
            &["gpt-4o".to_string(), "llama-3-70b".to_string()],
        )
        .await
        .unwrap();
        let router = crate::build_router(state);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/models", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let models = json["providers"][0]["models"].as_array().expect("models array");
        assert_eq!(models.len(), 2, "the host advertises two models");
        assert_eq!(models[0]["id"], "gpt-4o");
        assert_eq!(models[0]["enabled"], true);

        let update = |model: &str, body: &str| {
            post_with_cookie(
                &format!("/v1/dashboard/api/providers/{provider_id}/models/{model}"),
                &token,
                body,
            )
        };
        let disabled = router
            .clone()
            .oneshot(update("gpt-4o", r#"{"enabled":false}"#))
            .await
            .unwrap();
        assert_eq!(disabled.status(), axum::http::StatusCode::OK);
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/models", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let gpt4o = json["providers"][0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "gpt-4o")
            .unwrap();
        assert_eq!(gpt4o["enabled"], false);

        // A model the provider does not advertise -> 400.
        let unknown = router
            .clone()
            .oneshot(update("not-a-model", r#"{"enabled":false}"#))
            .await
            .unwrap();
        assert_eq!(unknown.status(), axum::http::StatusCode::BAD_REQUEST);

        // A member cannot change model selection.
        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{provider_id}/models/gpt-4o"),
                &member_token,
                r#"{"enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn provider_models_are_fetched_live_from_the_host() {
        // A tiny mock host exposing GET /v1/models in OpenAI shape.
        let host = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(json!({"data":[{"id":"llama-3-70b"},{"id":"mixtral"}]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, host).await.unwrap() });

        let (state, token) = tenant_role_session("owner").await;
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        let provider_id = crate::providers::ProviderStore::insert(
            &*state.provider_store,
            tenant_id,
            &crate::providers::NewProvider {
                kind: "openai-compatible".to_string(),
                name: "host".to_string(),
                base_url: format!("http://{addr}/v1"),
                key: "sk-host".to_string(),
                metadata: Default::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .unwrap();
        let router = crate::build_router(state);
        let response = router
            .clone()
            .oneshot(request_with_cookie(
                &format!("/v1/dashboard/api/providers/{provider_id}/models"),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(response).await;
        let ids: Vec<&str> = json["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["llama-3-70b", "mixtral"]);
        // The discovery is cached, so the tenant-wide list now has them.
        let listed = router
            .oneshot(request_with_cookie("/v1/dashboard/api/models", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(json["providers"][0]["models"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn owner_sets_the_default_provider_and_members_cannot() {
        let (state, token) = tenant_role_session("owner").await;
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        let new = |kind: &str, name: &str| crate::providers::NewProvider {
            kind: kind.to_string(),
            name: name.to_string(),
            base_url: "https://api.test/v1".to_string(),
            key: "sk-test-ab12".to_string(),
            metadata: Default::default(),
            spend_limit_cents: 0,
        };
        let first = crate::providers::ProviderStore::insert(
            &*state.provider_store,
            tenant_id,
            &new("deepseek", "First"),
        )
        .await
        .unwrap();
        let second = crate::providers::ProviderStore::insert(
            &*state.provider_store,
            tenant_id,
            &new("openai", "Second"),
        )
        .await
        .unwrap();
        let router = crate::build_router(state);

        let default_id = |json: &Value| {
            json["providers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["is_default"] == true)
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(default_id(&json), first.to_string(), "first insert is the default");

        let set = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{second}/default"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(set.status(), axum::http::StatusCode::OK);
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(default_id(&json), second.to_string());

        // Unknown provider -> 404; a member cannot set the default.
        let missing = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers/99999999-9999-9999-9999-999999999999/default",
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{second}/default"),
                &member_token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn owner_rotates_a_provider_and_members_cannot() {
        let (state, token) = tenant_role_session("owner").await;
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        let id = state
            .provider_store
            .insert(
                tenant_id,
                &crate::providers::NewProvider {
                    kind: "openai".to_string(),
                    name: "Mine".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    key: "sk-old-1111".to_string(),
                    metadata: Default::default(),
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("seed");
        let router = crate::build_router(state);

        // The list exposes the last-updated date + metadata (AC2).
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(listed).await;
        assert!(json["providers"][0]["updated_at"].is_string());
        assert!(json["providers"][0]["metadata"].is_object());

        // Rotate the key + attach org/headers (AC1/AC5).
        let updated = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{id}"),
                &token,
                r#"{"key":"sk-new-2222","metadata":{"org":"org-2","headers":{"X-Trace":"on"}}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(updated.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(updated).await["updated"], true);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(json["providers"][0]["key_hint"], "\u{2026}2222");
        assert_eq!(json["providers"][0]["metadata"]["org"], "org-2");
        assert_eq!(json["providers"][0]["metadata"]["headers"]["X-Trace"], "on");

        // Unknown id -> 404; a member cannot rotate at all.
        let missing = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers/99999999-9999-9999-9999-999999999999",
                &token,
                r#"{"key":"sk-x-1234"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{id}"),
                &member_token,
                r#"{"key":"sk-x-1234"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-039: tenant members API (session-scoped, owner manages) ---

    /// The members endpoint (ITEM-039) is scoped to the signed-in user's own
    /// tenant. These tests use owner/member sessions on the seeded tenant.

    #[tokio::test]
    async fn owner_invites_lists_and_removes_members() {
        let (state, token) = owner_with_member_state().await;
        let router = crate::build_router(state);

        // Owner invites a teammate by GitHub login (claim-based).
        let invite = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members",
                &token,
                r#"{"github_login":"carol"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(invite.status(), axum::http::StatusCode::OK, "owner invites");

        // The members view shows owner + member + the pending invite.
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        assert_eq!(view.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(view).await;
        assert_eq!(json["invites"], serde_json::json!(["carol"]));
        let logins: Vec<&str> = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["github_login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec!["t11", "alice"], "owner + member listed");
        let alice_id = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["github_login"] == "alice")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Owner removes alice; her membership (and invite, if any) is gone.
        let remove = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/members/{alice_id}/remove"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(remove.status(), axum::http::StatusCode::OK);
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        let json: Value = body_json(view).await;
        let logins: Vec<&str> = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["github_login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec!["t11"], "alice removed");
    }

    /// Owner t11 + member alice on the seeded tenant 2222….
    async fn owner_with_member_state() -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let owner = users.upsert_github_user(11, "t11", false).await.expect("ok");
        let alice = users.upsert_github_user(21, "alice", false).await.expect("ok");
        let tid = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        users.add_membership(tid, owner.id, "owner").await.expect("ok");
        users.add_membership(tid, alice.id, "member").await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            owner.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    #[tokio::test]
    async fn member_sessions_view_members_but_cannot_invite_or_remove() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        assert_eq!(view.status(), axum::http::StatusCode::OK, "members may view");
        let invite = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members",
                &token,
                r#"{"github_login":"bob"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(
            invite.status(),
            axum::http::StatusCode::FORBIDDEN,
            "members cannot invite (owner only)"
        );
        let remove = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members/00000000-0000-0000-0000-000000000000/remove",
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(remove.status(), axum::http::StatusCode::FORBIDDEN);
    }
// --- ITEM-081: per-tenant tier pairs ------------------------------------

/// Seed a provider advertising `models`, all enabled, for the seeded tenant.
async fn seed_models(state: &std::sync::Arc<crate::AppState>, models: &[&str]) -> uuid::Uuid {
    let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
    let provider_id = state
        .provider_store
        .insert(
            tenant_id,
            &crate::providers::NewProvider {
                kind: "local".to_string(),
                name: "lab".to_string(),
                base_url: "http://127.0.0.1:11434/v1".to_string(),
                key: String::new(),
                metadata: crate::providers::ProviderMetadata::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .expect("provider seeded");
    let advertised: Vec<String> = models.iter().map(|model| model.to_string()).collect();
    state
        .provider_store
        .record_models(provider_id, &advertised)
        .await
        .expect("models recorded");
    provider_id
}

async fn json_of(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn an_owner_sets_tier_pairs_and_the_response_says_which_are_live() {
    let (state, token) = tenant_role_session("owner").await;
    seed_models(&state, &["gpt-4o", "qwen3:8b"]).await;
    let router = crate::build_router(state);

    let empty = router
        .clone()
        .oneshot(request_with_cookie("/v1/dashboard/api/routing", &token))
        .await
        .unwrap();
    assert_eq!(empty.status(), axum::http::StatusCode::OK);

    let saved = router
        .clone()
        .oneshot(post_with_cookie(
            "/v1/dashboard/api/routing/tier-pairs",
            &token,
            r#"{"tier_pairs":[{"frontier":"gpt-4o","cheap":"qwen3:8b"}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), axum::http::StatusCode::OK);
    let saved = json_of(saved).await;
    assert_eq!(saved["tier_pairs"][0]["frontier"], "gpt-4o");
    assert_eq!(saved["tier_pairs"][0]["cheap"], "qwen3:8b");
    assert_eq!(saved["tier_pairs"][0]["active"], true);

    let listed = router
        .clone()
        .oneshot(request_with_cookie("/v1/dashboard/api/routing", &token))
        .await
        .unwrap();
    let listed = json_of(listed).await;
    assert_eq!(listed["tier_pairs"][0]["cheap"], "qwen3:8b");
    assert_eq!(listed["tier_pairs"][0]["active"], true);
}

#[tokio::test]
async fn a_pair_naming_a_model_the_tenant_cannot_route_to_is_refused() {
    let (state, token) = tenant_role_session("owner").await;
    seed_models(&state, &["gpt-4o"]).await;
    let router = crate::build_router(state);
    let response = router
        .oneshot(post_with_cookie(
            "/v1/dashboard/api/routing/tier-pairs",
            &token,
            r#"{"tier_pairs":[{"frontier":"gpt-4o","cheap":"qwen3:8b"}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let body = json_of(response).await;
    assert_eq!(body["error"]["type"], "tier_pair_unknown_model");
}

#[tokio::test]
async fn a_disabled_cheap_model_makes_the_pair_inactive_not_silent() {
    let (state, token) = tenant_role_session("owner").await;
    let provider_id = seed_models(&state, &["gpt-4o", "qwen3:8b"]).await;
    state
        .budget_store
        .set_tier_pairs(
            uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(),
            &[crate::router::TierPair {
                frontier: "gpt-4o".into(),
                cheap: "qwen3:8b".into(),
            }],
        )
        .await
        .unwrap();
    state
        .model_store
        .set_enabled(provider_id, "qwen3:8b", false)
        .await
        .unwrap();
    let listed = crate::build_router(state)
        .oneshot(request_with_cookie("/v1/dashboard/api/routing", &token))
        .await
        .unwrap();
    let listed = json_of(listed).await;
    assert_eq!(
        listed["tier_pairs"][0]["active"], false,
        "reported, never dropped"
    );
}

#[tokio::test]
async fn a_member_cannot_change_tier_pairs() {
    let (state, token) = tenant_role_session("member").await;
    seed_models(&state, &["gpt-4o", "qwen3:8b"]).await;
    let response = crate::build_router(state)
        .oneshot(post_with_cookie(
            "/v1/dashboard/api/routing/tier-pairs",
            &token,
            r#"{"tier_pairs":[{"frontier":"gpt-4o","cheap":"qwen3:8b"}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}

    // --- ITEM-082: attaching a keyless local provider -----------------------

    #[tokio::test]
    async fn an_owner_attaches_a_keyless_local_provider() {
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state.clone());
        let response = router
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers",
                &token,
                r#"{"kind":"local","base_url":"http://127.0.0.1:11434/v1","key":""}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let created = json_of(response).await;
        assert!(created["id"].is_string());

        let providers = state
            .provider_store
            .list(uuid::Uuid::parse_str(SEEDED_TENANT).unwrap())
            .await
            .unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].kind, "local");
        assert_eq!(providers[0].base_url, "http://127.0.0.1:11434/v1");
    }

    #[tokio::test]
    async fn a_cloud_provider_still_needs_a_key() {
        let (state, token) = tenant_role_session("owner").await;
        let response = crate::build_router(state)
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers",
                &token,
                r#"{"kind":"openai","base_url":"https://api.openai.com/v1","key":""}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(json_of(response).await["error"]["type"], "invalid_key");
    }

    #[tokio::test]
    async fn a_keyless_local_probe_is_not_refused_for_its_missing_key() {
        // Unreachable is fine — the point is that the empty key is not a 400.
        let (state, token) = tenant_role_session("owner").await;
        let response = crate::build_router(state)
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers/test",
                &token,
                r#"{"kind":"local","base_url":"http://127.0.0.1:1/v1","key":""}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(json_of(response).await["ok"], false);
    }

    #[tokio::test]
    async fn a_local_providers_models_are_shown_as_free_not_unpriced() {
        // ITEM-082 AC4: attaching Ollama must not label its models with cloud
        // prices (or "unpriced") — local inference costs nothing.
        let (state, token) = tenant_role_session("owner").await;
        seed_models(&state, &["qwen3:8b"]).await;
        let listed = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/models", &token))
            .await
            .unwrap();
        let listed = json_of(listed).await;
        let price = &listed["providers"][0]["models"][0]["price"];
        assert_eq!(price["local"], true);
        assert_eq!(price["priced"], true);
        assert_eq!(price["input_usd_per_million"], "0");
        assert_eq!(price["output_usd_per_million"], "0");
    }

    #[tokio::test]
    async fn a_storage_failure_is_logged_not_echoed_to_the_caller() {
        // dashboard_state() points at a lazy pool that cannot connect; the
        // response must be a generic 500, not a transcript of the failure.
        let (state, token) = tenant_role_session("owner").await;
        let response = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/summary?days=7", &token))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let body = json_of(response).await;
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert_eq!(body["error"]["type"], "dashboard_error");
        assert!(
            !message.contains("connection") && !message.to_lowercase().contains("postgres"),
            "storage detail leaked to the caller: {message}"
        );
    }

    #[tokio::test]
    async fn the_performance_report_is_session_scoped() {
        let anonymous = crate::build_router(dashboard_state())
            .oneshot(get("/v1/dashboard/api/performance?days=7", None))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), axum::http::StatusCode::UNAUTHORIZED);

        let (state, token) = tenant_role_session("owner").await;
        let scoped = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/performance?days=7", &token))
            .await
            .unwrap();
        assert_ne!(
            scoped.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "a session reaches the query (the lazy pool then fails it)"
        );
    }

    #[tokio::test]
    async fn the_dataset_report_is_session_scoped() {
        let anonymous = crate::build_router(dashboard_state())
            .oneshot(get("/v1/dashboard/api/datasets?days=30", None))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), axum::http::StatusCode::UNAUTHORIZED);

        let (state, token) = tenant_role_session("owner").await;
        let scoped = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/datasets?days=30", &token))
            .await
            .unwrap();
        assert_ne!(scoped.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_capture_switch_round_trips_through_the_config_api() {
        let (state, token) = tenant_role_session("owner").await;
        let body = r#"{"budget_limit_cents":0,"cheap_fallback_model":null,
            "guard_enabled":true,"router_enabled":true,"router_threshold":0.35,
            "capture_payloads":false,"payload_retention_days":7}"#;
        let saved = crate::build_router(state.clone())
            .oneshot(post_with_cookie("/v1/dashboard/api/config", &token, body))
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        let listed = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        let listed = serde_json::from_slice::<Value>(
            &axum::body::to_bytes(listed.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        assert_eq!(listed["capture_payloads"], false);
        assert_eq!(listed["payload_retention_days"], 7);
    }

    #[tokio::test]
    async fn the_dataset_export_is_owner_only() {
        let anonymous = crate::build_router(dashboard_state())
            .oneshot(get(
                "/v1/dashboard/api/datasets/export?prompt_hash=p&model=m",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), axum::http::StatusCode::UNAUTHORIZED);

        let (state, token) = tenant_role_session("member").await;
        let member = crate::build_router(state)
            .oneshot(request_with_cookie(
                "/v1/dashboard/api/datasets/export?prompt_hash=p&model=m",
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_transaction_report_is_session_scoped() {
        let anonymous = crate::build_router(dashboard_state())
            .oneshot(get("/v1/dashboard/api/sessions?days=7", None))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), axum::http::StatusCode::UNAUTHORIZED);

        let (state, token) = tenant_role_session("owner").await;
        let scoped = crate::build_router(state)
            .oneshot(request_with_cookie("/v1/dashboard/api/sessions?days=7", &token))
            .await
            .unwrap();
        assert_ne!(scoped.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
