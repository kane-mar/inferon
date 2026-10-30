    use super::*;
    use crate::TenantStore;

    fn store() -> InMemoryUserStore {
        InMemoryUserStore::default()
    }

    #[tokio::test]
    async fn first_sign_in_creates_a_user_row() {
        let store = store();
        let user = store
            .upsert_github_user(42, "kane", false)
            .await
            .expect("store ok");
        assert_eq!(user.github_id, 42);
        assert_eq!(user.github_login, "kane");
        let found = store.find_by_id(user.id).await.expect("store ok");
        assert_eq!(found.map(|u| u.github_login), Some("kane".to_string()));
    }

    #[tokio::test]
    async fn repeated_sign_ins_update_login_and_keep_one_row() {
        let store = store();
        let first = store.upsert_github_user(42, "kane", false).await.expect("store ok");
        let second = store.upsert_github_user(42, "KaneMar", false).await.expect("store ok");
        assert_eq!(second.id, first.id, "same GitHub id keeps the same user row");
        assert_eq!(second.github_login, "KaneMar");
        // Still exactly one row for that GitHub id.
        let found = store.find_by_id(first.id).await.expect("store ok");
        assert_eq!(found.map(|u| u.github_login), Some("KaneMar".to_string()));
    }

    #[tokio::test]
    async fn different_github_ids_are_distinct_users() {
        let store = store();
        let a = store.upsert_github_user(1, "alice", false).await.expect("store ok");
        let b = store.upsert_github_user(2, "bob", false).await.expect("store ok");
        assert_ne!(a.id, b.id);
        assert!(store.find_by_id(a.id).await.expect("ok").is_some());
        assert!(store.find_by_id(b.id).await.expect("ok").is_some());
    }

    #[tokio::test]
    async fn find_by_id_missing_returns_none() {
        let store = store();
        assert_eq!(
            store.find_by_id(Uuid::new_v4()).await.expect("store ok"),
            None
        );
    }

    // --- ITEM-036: tenant membership (migration 008) ---

    #[tokio::test]
    async fn add_membership_makes_the_user_a_tenant_member() {
        let store = store();
        let user = store.upsert_github_user(42, "kane", false).await.expect("ok");
        let tenant_id = Uuid::new_v4();
        assert_eq!(store.tenant_for_user(user.id).await.expect("ok"), None, "no membership yet");
        store.add_membership(tenant_id, user.id, "owner").await.expect("ok");
        assert_eq!(
            store.tenant_for_user(user.id).await.expect("ok"),
            Some((tenant_id, "owner".to_string()))
        );
    }

    #[tokio::test]
    async fn membership_add_is_idempotent_and_unknown_users_have_none() {
        let store = store();
        let user = store.upsert_github_user(7, "bob", false).await.expect("ok");
        let tenant_id = Uuid::new_v4();
        store.add_membership(tenant_id, user.id, "member").await.expect("ok");
        store.add_membership(tenant_id, user.id, "member").await.expect("ok");
        assert_eq!(
            store.tenant_for_user(user.id).await.expect("ok"),
            Some((tenant_id, "member".to_string())),
            "role kept on re-add"
        );
        assert_eq!(store.tenant_for_user(Uuid::new_v4()).await.expect("ok"), None);
    }

    // --- ITEM-037: platform-operator flag (migration 009) ---

    #[tokio::test]
    async fn upsert_carries_the_operator_flag_and_re_sign_in_is_authoritative() {
        let store = store();
        let first = store.upsert_github_user(42, "octocat", true).await.expect("ok");
        assert!(first.is_operator, "flagged operator on insert");
        // Same GitHub id signs in again while in the operator env list.
        let again = store.upsert_github_user(42, "octocat", true).await.expect("ok");
        assert!(again.is_operator);
        assert_eq!(again.id, first.id, "one row");
        // Env is authoritative at sign-in: a later sign-in outside the list demotes.
        let demoted = store.upsert_github_user(42, "octocat", false).await.expect("ok");
        assert!(!demoted.is_operator, "re-sign-in outside the env list demotes");
        assert_eq!(demoted.id, first.id);
    }

    #[tokio::test]
    async fn non_operators_are_not_flagged_and_the_flag_survives_find() {
        let store = store();
        let user = store.upsert_github_user(7, "bob", false).await.expect("ok");
        assert!(!user.is_operator);
        let found = store.find_by_id(user.id).await.expect("ok").expect("present");
        assert!(!found.is_operator);
    }

    #[tokio::test]
    async fn promote_logins_backfills_existing_users_at_boot() {
        let store = store();
        let user = store.upsert_github_user(7, "boss", false).await.expect("ok");
        let other = store.upsert_github_user(8, "dev", false).await.expect("ok");
        let promoted = store.promote_logins(&["boss"]).await.expect("ok");
        assert_eq!(promoted, 1, "only the listed login is promoted");
        assert!(store.find_by_id(user.id).await.expect("ok").unwrap().is_operator);
        assert!(!store.find_by_id(other.id).await.expect("ok").unwrap().is_operator);
        // Idempotent at boot: nothing left to promote.
        assert_eq!(store.promote_logins(&["boss", "nobody"]).await.expect("ok"), 0);
    }

    // --- ITEM-039: claim-based member invites (migration 010) ---

    #[tokio::test]
    async fn an_invite_is_claimed_once_when_the_user_signs_in() {
        let store = store();
        let tenant_id = Uuid::new_v4();
        store.invite_member(tenant_id, "alice").await.expect("ok");
        store.invite_member(tenant_id, "alice").await.expect("ok"); // idempotent
        assert_eq!(store.list_invites(tenant_id).await.expect("ok"), vec!["alice".to_string()]);
        assert_eq!(store.claim_invite("alice").await.expect("ok"), Some(tenant_id));
        assert_eq!(store.claim_invite("alice").await.expect("ok"), None, "claimed exactly once");
        // The invite list is drained after the claim.
        assert!(store.list_invites(tenant_id).await.expect("ok").is_empty());
    }

    #[tokio::test]
    async fn list_members_joins_logins_and_remove_member_spares_the_owner() {
        let store = store();
        let tenant_id = Uuid::new_v4();
        let owner = store.upsert_github_user(1, "boss", false).await.expect("ok");
        let alice = store.upsert_github_user(2, "alice", false).await.expect("ok");
        store.add_membership(tenant_id, owner.id, "owner").await.expect("ok");
        store.add_membership(tenant_id, alice.id, "member").await.expect("ok");
        let members = store.list_members(tenant_id).await.expect("ok");
        let logins: Vec<&str> = members.iter().map(|(_, login, _)| login.as_str()).collect();
        assert_eq!(logins, vec!["boss", "alice"]);
        let owner_role = members.iter().find(|(_, l, _)| l == "boss").map(|(_, _, r)| r.clone());
        assert_eq!(owner_role.as_deref(), Some("owner"));
        // Removing the owner is refused; removing alice works and ends her scope.
        let owner_id = members.iter().find(|(_, l, _)| l == "boss").unwrap().0;
        assert!(!store.remove_member(tenant_id, owner_id).await.expect("ok"), "owner protected");
        let alice_id = members.iter().find(|(_, l, _)| l == "alice").unwrap().0;
        assert!(store.remove_member(tenant_id, alice_id).await.expect("ok"));
        assert_eq!(store.tenant_for_user(alice.id).await.expect("ok"), None, "scope ended");
        assert_eq!(store.list_members(tenant_id).await.expect("ok").len(), 1);
    }

    #[tokio::test]
    async fn removing_a_member_also_drops_their_pending_invite() {
        let store = store();
        let tenant_id = Uuid::new_v4();
        let owner = store.upsert_github_user(1, "boss", false).await.expect("ok");
        let alice = store.upsert_github_user(2, "alice", false).await.expect("ok");
        store.add_membership(tenant_id, owner.id, "owner").await.expect("ok");
        store.add_membership(tenant_id, alice.id, "member").await.expect("ok");
        store.invite_member(tenant_id, "alice").await.expect("ok");
        store.remove_member(tenant_id, alice.id).await.expect("ok");
        assert_eq!(store.claim_invite("alice").await.expect("ok"), None, "invite gone too");
    }

    // --- ITEM-035 AC1/AC3: OAuth authorize URL + query encoding ---

    fn config() -> OauthConfig {
        OauthConfig {
            client_id: "client-123".to_string(),
            client_secret: "secret".to_string(),
            authorize_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            api_url: "https://api.github.com".to_string(),
        }
    }

    #[test]
    fn authorize_url_carries_credentials_state_and_scope() {
        let url = authorize_url(&config(), "https://inferon.example/auth/callback", "nonce123");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("client_id=client-123"), "client id: {url}");
        assert!(url.contains("state=nonce123"), "state: {url}");
        assert!(url.contains("scope=read:user"), "scope: {url}");
        // The redirect URI is percent-encoded (it is a URL inside a query).
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Finferon.example%2Fauth%2Fcallback"),
            "encoded redirect: {url}"
        );
    }

    #[test]
    fn encode_query_keeps_only_unreserved_characters() {
        assert_eq!(encode_query("abc-._~123"), "abc-._~123");
        assert_eq!(encode_query("a b&c=/d"), "a%20b%26c%3D%2Fd");
        assert_eq!(encode_query(""), "");
    }

    // --- ITEM-035 AC1-AC4: /auth/* behavior through the router ---

    use axum::{
        body::Body,
        http::{header, Request},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    /// Recording fake: answers a fixed GitHub identity and remembers the code
    /// + redirect_uri it was asked to exchange.
    #[derive(Default)]
    struct FakeGithub {
        calls: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl GithubClient for FakeGithub {
        async fn exchange_code(
            &self,
            code: &str,
            redirect_uri: &str,
        ) -> Result<GithubUser, String> {
            self.calls.lock().unwrap().push((code.to_string(), redirect_uri.to_string()));
            Ok(GithubUser {
                github_id: 42,
                login: "octocat".to_string(),
            })
        }
    }

    struct BrokenGithub;

    #[async_trait::async_trait]
    impl GithubClient for BrokenGithub {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
        ) -> Result<GithubUser, String> {
            Err("simulated upstream failure".to_string())
        }
    }

    const SECRET: &str = "test-session-secret";

    fn test_accounts(
        users: std::sync::Arc<dyn UserStore>,
        github: std::sync::Arc<dyn GithubClient>,
    ) -> Option<std::sync::Arc<Accounts>> {
        Some(std::sync::Arc::new(Accounts {
            oauth: OauthConfig {
                client_id: "client-123".to_string(),
                client_secret: "secret".to_string(),
                authorize_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                api_url: "https://api.github.com".to_string(),
            },
            session_secret: SECRET.to_string(),
            operator_logins: std::collections::HashSet::new(),
            users,
            github,
        }))
    }

    fn test_state_full(
        accounts: Option<std::sync::Arc<Accounts>>,
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
            operator_key: None,
            accounts,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        })
    }

    fn test_state(accounts: Option<std::sync::Arc<Accounts>>) -> std::sync::Arc<crate::AppState> {
        test_state_full(accounts, std::sync::Arc::new(crate::InMemoryTenantStore::default()))
    }

    fn request(uri: &str, method: &str, cookies: &[(&str, &str)], secure: bool) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "inferon.example");
        if secure {
            builder = builder.header("x-forwarded-proto", "https");
        }
        for (name, value) in cookies {
            builder = builder.header("cookie", format!("{name}={value}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// name=value pairs from every Set-Cookie header on the response.
    fn set_cookies(response: &axum::response::Response) -> std::collections::HashMap<String, String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|raw| {
                let (name, value) = raw.split_once('=')?;
                let value = value.split(';').next().unwrap_or("").to_string();
                Some((name.to_string(), value))
            })
            .collect()
    }
    #[tokio::test]
    async fn login_without_oauth_configuration_answers_503() {
        let state = test_state(None);
        let response = crate::build_router(state)
            .oneshot(request("/auth/login", "GET", &[], false))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let json = body_json(response).await;
        assert_eq!(json["error"]["type"], "oauth_not_configured");
    }

    #[tokio::test]
    async fn login_redirects_to_github_with_state_and_secure_cookie() {
        let users: std::sync::Arc<dyn UserStore> = std::sync::Arc::new(InMemoryUserStore::default());
        let github: std::sync::Arc<dyn GithubClient> = std::sync::Arc::new(FakeGithub::default());
        let state = test_state(test_accounts(users, github));
        let response = crate::build_router(state)
            .oneshot(request("/auth/login", "GET", &[], true))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(location.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(location.contains("client_id=client-123"));
        assert!(location.contains("scope=read:user"));
        assert!(
            location.contains("redirect_uri=https%3A%2F%2Finferon.example%2Fauth%2Fcallback"),
            "{location}"
        );
        let raw_cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        let state_cookie = raw_cookies
            .iter()
            .find(|c| c.starts_with("inferon_oauth_state="))
            .expect("state cookie");
        assert!(state_cookie.contains("HttpOnly") && state_cookie.contains("Secure"));
    }

    /// Run login -> callback with the freshly issued state cookie; returns the
    /// callback response.
    async fn oauth_dance(router: &axum::Router, code: &str) -> axum::response::Response {
        let login = router
            .clone()
            .oneshot(request("/auth/login", "GET", &[], false))
            .await
            .unwrap();
        assert_eq!(login.status(), axum::http::StatusCode::FOUND);
        let nonce = login.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string();
        let state_cookie = set_cookies(&login)
            .get(crate::session::OAUTH_STATE_COOKIE)
            .expect("state cookie")
            .to_string();
        router
            .clone()
            .oneshot(request(
                &format!("/auth/callback?code={code}&state={nonce}"),
                "GET",
                &[(crate::session::OAUTH_STATE_COOKIE, &state_cookie)],
                false,
            ))
            .await
            .unwrap()
    }

    /// The raw `sk-inferon-…` key inside an HTML body (the success screen).
    #[tokio::test]
    async fn first_sign_in_provisions_the_tenant_and_redirects_to_the_frontend() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let tenants: std::sync::Arc<crate::InMemoryTenantStore> =
            std::sync::Arc::new(crate::InMemoryTenantStore::default());
        let state = test_state_full(
            test_accounts(users.clone(), std::sync::Arc::new(FakeGithub::default())),
            tenants.clone(),
        );
        let router = crate::build_router(state);

        let callback = oauth_dance(&router, "the-code").await;
        assert_eq!(
            callback.status(),
            axum::http::StatusCode::FOUND,
            "redirects to the frontend, not an HTML screen"
        );
        assert_eq!(callback.headers()[header::LOCATION], "/dashboard/onboarding");
        let cookies = set_cookies(&callback);

        // A session cookie was set for the new user.
        let token = cookies.get(crate::session::SESSION_COOKIE).expect("session cookie").to_string();
        let user_id =
            crate::session::verify_session(&token, SECRET, now_secs()).expect("fresh token");
        let user = users.find_by_id(user_id).await.expect("ok").expect("user present");
        assert_eq!(user.github_login, "octocat");

        // The user owns exactly one auto-provisioned tenant. It is **keyless**:
        // the platform's shared key is not a pass-through — tenants add their
        // own provider (env UPSTREAM_MASTER_KEY is only used by the explicit
        // operator onboarding path, scripts/seed-tenant.sh).
        let (tenant_id, role) = users
            .tenant_for_user(user.id)
            .await
            .expect("ok")
            .expect("owner membership");
        assert_eq!(role, "owner");
        let tenant = tenants
            .tenant_by_id(tenant_id)
            .await
            .expect("ok")
            .expect("provisioned tenant");
        assert_eq!(tenant.upstream_openai_key, None, "self-serve is keyless");

        // No key is minted server-side — the frontend's keys page mints it and
        // shows it once. The backend renders no HTML and reveals no key.
        assert!(
            tenants.list_api_keys(tenant_id).await.expect("ok").is_empty(),
            "the backend mints no first key; the frontend does"
        );
    }

    #[tokio::test]
    async fn an_invited_sign_in_joins_the_tenants_tenant_as_member() {
        // ITEM-039: an owner invited login "octocat"; when that user signs in
        // they join the invite's tenant as member — no provisioning, no key.
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let tenants: std::sync::Arc<crate::InMemoryTenantStore> =
            std::sync::Arc::new(crate::InMemoryTenantStore::default());
        let invited_tenant = Uuid::new_v4();
        tenants.insert_tenant(Some("sk-upstream")).await.expect("ok"); // unrelated
        users
            .invite_member(invited_tenant, "octocat")
            .await
            .expect("ok");
        let state = test_state_full(
            test_accounts(users.clone(), std::sync::Arc::new(FakeGithub::default())),
            tenants.clone(),
        );
        let router = crate::build_router(state);

        let callback = oauth_dance(&router, "invited-code").await;
        assert_eq!(
            callback.status(),
            axum::http::StatusCode::FOUND,
            "an invited user lands on the dashboard, not the signup screen"
        );
        assert_eq!(callback.headers()[header::LOCATION], "/dashboard");
        let token = set_cookies(&callback)
            .get(crate::session::SESSION_COOKIE)
            .expect("session cookie")
            .to_string();
        let user_id =
            crate::session::verify_session(&token, SECRET, now_secs()).expect("valid token");
        assert_eq!(
            users.tenant_for_user(user_id).await.expect("ok"),
            Some((invited_tenant, "member".to_string())),
            "joined the invited tenant as member"
        );
        // The invite is consumed and no new tenant/key was provisioned.
        assert!(users.list_invites(invited_tenant).await.expect("ok").is_empty());
    }

    #[tokio::test]
    async fn returning_sign_in_lands_on_the_dashboard_without_another_key() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let tenants: std::sync::Arc<crate::InMemoryTenantStore> =
            std::sync::Arc::new(crate::InMemoryTenantStore::default());
        let state = test_state_full(
            test_accounts(users.clone(), std::sync::Arc::new(FakeGithub::default())),
            tenants.clone(),
        );
        let router = crate::build_router(state);

        // First sign-in: provisions the tenant (no key) and redirects.
        let first = oauth_dance(&router, "code-1").await;
        assert_eq!(first.status(), axum::http::StatusCode::FOUND);
        assert_eq!(first.headers()[header::LOCATION], "/dashboard/onboarding");
        let user_id = crate::session::verify_session(
            set_cookies(&first)
                .get(crate::session::SESSION_COOKIE)
                .expect("session")
                .as_str(),
            SECRET,
            now_secs(),
        )
        .expect("valid");
        let (tenant_id, _) = users.tenant_for_user(user_id).await.expect("ok").expect("membership");

        // Second sign-in (same GitHub user): straight to the dashboard.
        let second = oauth_dance(&router, "code-2").await;
        assert_eq!(second.status(), axum::http::StatusCode::FOUND);
        assert_eq!(second.headers()[header::LOCATION], "/dashboard");
        // Still one tenant, one membership — sign-in never re-provisions.
        assert_eq!(users.tenant_for_user(user_id).await.expect("ok"), Some((tenant_id, "owner".to_string())));
    }

    #[tokio::test]
    async fn me_answers_401_without_forged_or_unknown_session() {
        let users: std::sync::Arc<dyn UserStore> = std::sync::Arc::new(InMemoryUserStore::default());
        let github: std::sync::Arc<dyn GithubClient> = std::sync::Arc::new(FakeGithub::default());
        let state = test_state(test_accounts(users, github));
        let router = crate::build_router(state);

        let no_cookie = router
            .clone()
            .oneshot(request("/auth/me", "GET", &[], false))
            .await
            .unwrap();
        assert_eq!(no_cookie.status(), axum::http::StatusCode::UNAUTHORIZED, "no cookie");

        // A real token for a user that does not exist is signed out.
        let token = crate::session::issue_session(Uuid::new_v4(), SECRET, 1_000, 3600);
        let unknown = router
            .clone()
            .oneshot(request(
                "/auth/me",
                "GET",
                &[(crate::session::SESSION_COOKIE, &token)],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(unknown.status(), axum::http::StatusCode::UNAUTHORIZED, "unknown user");

        // A forged cookie (tampered mac) is signed out.
        let forged = format!("{}0", &token[..token.len() - 1]);
        let forged_resp = router
            .clone()
            .oneshot(request(
                "/auth/me",
                "GET",
                &[(crate::session::SESSION_COOKIE, &forged)],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(forged_resp.status(), axum::http::StatusCode::UNAUTHORIZED, "forged cookie");
    }

    #[tokio::test]
    async fn me_returns_the_signed_in_user_including_the_operator_flag() {
        let store = std::sync::Arc::new(InMemoryUserStore::default());
        let operator = store.upsert_github_user(42, "kane-mar", true).await.expect("ok");
        let member = store.upsert_github_user(43, "dev", false).await.expect("ok");
        let users: std::sync::Arc<dyn UserStore> = store;
        let github: std::sync::Arc<dyn GithubClient> = std::sync::Arc::new(FakeGithub::default());
        let router = crate::build_router(test_state(test_accounts(users, github)));

        let op_token = crate::session::issue_session(operator.id, SECRET, now_secs(), 3600);
        let op = router
            .clone()
            .oneshot(request(
                "/auth/me",
                "GET",
                &[(crate::session::SESSION_COOKIE, &op_token)],
                true,
            ))
            .await
            .unwrap();
        assert_eq!(op.status(), axum::http::StatusCode::OK);
        let op_json = body_json(op).await;
        assert_eq!(op_json["github_login"], "kane-mar");
        assert_eq!(op_json["is_operator"], true, "operator flag is exposed to the UI");

        let member_token = crate::session::issue_session(member.id, SECRET, now_secs(), 3600);
        let member_resp = router
            .oneshot(request(
                "/auth/me",
                "GET",
                &[(crate::session::SESSION_COOKIE, &member_token)],
                true,
            ))
            .await
            .unwrap();
        assert_eq!(member_resp.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(member_resp).await["is_operator"], false);
    }

    #[tokio::test]
    async fn callback_rejects_a_state_mismatch_without_a_session() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let state = test_state(test_accounts(users.clone(), std::sync::Arc::new(FakeGithub::default())));
        let router = crate::build_router(state);
        let callback = router
            .clone()
            .oneshot(request(
                "/auth/callback?code=the-code&state=wrong-nonce",
                "GET",
                &[(crate::session::OAUTH_STATE_COOKIE, "some-other-state")],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(callback.status(), axum::http::StatusCode::BAD_REQUEST);
        assert!(
            !set_cookies(&callback).contains_key(crate::session::SESSION_COOKIE),
            "no session on mismatch"
        );
        let json = body_json(callback).await;
        assert_eq!(json["error"]["type"], "oauth_state_mismatch");
    }

    #[tokio::test]
    async fn exchange_failure_answers_502_and_records_no_user() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let state = test_state(test_accounts(
            users.clone(),
            std::sync::Arc::new(BrokenGithub),
        ));
        let router = crate::build_router(state);
        let callback = router
            .clone()
            .oneshot(request(
                "/auth/callback?code=bad&state=s",
                "GET",
                &[(crate::session::OAUTH_STATE_COOKIE, "s")],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(callback.status(), axum::http::StatusCode::BAD_GATEWAY);
        let json = body_json(callback).await;
        assert_eq!(json["error"]["type"], "github_exchange_failed");
        // Nothing was upserted.
        let guard = users.users.lock().unwrap();
        assert!(guard.is_empty());
    }

    #[tokio::test]
    async fn logout_expires_the_session_cookie() {
        let users: std::sync::Arc<dyn UserStore> = std::sync::Arc::new(InMemoryUserStore::default());
        let state = test_state(test_accounts(users, std::sync::Arc::new(FakeGithub::default())));
        let response = crate::build_router(state)
            .oneshot(request("/auth/logout", "POST", &[], true))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
        assert_eq!(response.headers()[header::LOCATION], "/dashboard");
        let raw = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with("inferon_session="))
            .expect("session clear cookie");
        assert!(raw.contains("Max-Age=0"), "expired: {raw}");
    }

    // --- callback branch coverage (drives the refactor of `callback`) ---

    type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

    /// A `UserStore` that fails one configured call; everything else delegates
    /// to a working in-memory store. Lets the tests drive `callback`'s error
    /// branches deterministically.
    #[derive(Clone, Default)]
    struct FailingUserStore {
        inner: InMemoryUserStore,
        fail: Option<&'static str>,
    }

    #[async_trait::async_trait]
    impl UserStore for FailingUserStore {
        async fn upsert_github_user(
            &self,
            github_id: i64,
            github_login: &str,
            is_operator: bool,
        ) -> StoreResult<User> {
            if self.fail == Some("upsert") {
                return Err("boom".into());
            }
            self.inner.upsert_github_user(github_id, github_login, is_operator).await
        }
        async fn promote_logins(&self, logins: &[&str]) -> StoreResult<usize> {
            self.inner.promote_logins(logins).await
        }
        async fn find_by_id(&self, user_id: Uuid) -> StoreResult<Option<User>> {
            self.inner.find_by_id(user_id).await
        }
        async fn tenant_for_user(&self, user_id: Uuid) -> StoreResult<Option<(Uuid, String)>> {
            if self.fail == Some("tenant_for_user") {
                return Err("boom".into());
            }
            self.inner.tenant_for_user(user_id).await
        }
        async fn add_membership(&self, tenant_id: Uuid, user_id: Uuid, role: &str) -> StoreResult<()> {
            if self.fail == Some("add_membership") {
                return Err("boom".into());
            }
            self.inner.add_membership(tenant_id, user_id, role).await
        }
        async fn invite_member(&self, tenant_id: Uuid, github_login: &str) -> StoreResult<()> {
            self.inner.invite_member(tenant_id, github_login).await
        }
        async fn claim_invite(&self, github_login: &str) -> StoreResult<Option<Uuid>> {
            if self.fail == Some("claim_invite") {
                return Err("boom".into());
            }
            self.inner.claim_invite(github_login).await
        }
        async fn list_members(&self, tenant_id: Uuid) -> StoreResult<Vec<(Uuid, String, String)>> {
            self.inner.list_members(tenant_id).await
        }
        async fn remove_member(&self, tenant_id: Uuid, member_id: Uuid) -> StoreResult<bool> {
            self.inner.remove_member(tenant_id, member_id).await
        }
        async fn list_invites(&self, tenant_id: Uuid) -> StoreResult<Vec<String>> {
            self.inner.list_invites(tenant_id).await
        }
    }

    /// Drive login -> callback against a `FailingUserStore`; `invite` is
    /// pre-claimed by the FakeGithub login "octocat".
    async fn callback_with_store(
        fail: Option<&'static str>,
        invite: bool,
    ) -> axum::response::Response {
        let store = std::sync::Arc::new(FailingUserStore {
            inner: InMemoryUserStore::default(),
            fail,
        });
        if invite {
            store
                .inner
                .invite_member(Uuid::new_v4(), "octocat")
                .await
                .expect("invite");
        }
        let users: std::sync::Arc<dyn UserStore> = store.clone();
        let state = test_state(test_accounts(users, std::sync::Arc::new(FakeGithub::default())));
        oauth_dance(&crate::build_router(state), "code").await
    }

    #[tokio::test]
    async fn callback_without_a_code_is_rejected() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let state = test_state(test_accounts(users, std::sync::Arc::new(FakeGithub::default())));
        let response = crate::build_router(state)
            .oneshot(request(
                "/auth/callback?state=s",
                "GET",
                &[(crate::session::OAUTH_STATE_COOKIE, "s")],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["error"]["type"], "oauth_callback_error");
    }

    #[tokio::test]
    async fn callback_clears_the_one_time_state_cookie() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let state = test_state(test_accounts(users, std::sync::Arc::new(FakeGithub::default())));
        let callback = oauth_dance(&crate::build_router(state), "c").await;
        let cleared = callback
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|c| c.starts_with("inferon_oauth_state=") && c.contains("Max-Age=0"));
        assert!(cleared, "the state cookie is one-time and must be cleared");
    }

    #[tokio::test]
    async fn a_failed_user_upsert_answers_500() {
        let response = callback_with_store(Some("upsert"), false).await;
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_json(response).await["error"]["type"], "user_store_error");
    }

    #[tokio::test]
    async fn a_failed_membership_lookup_answers_500() {
        let response = callback_with_store(Some("tenant_for_user"), false).await;
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_json(response).await["error"]["type"], "user_store_error");
    }

    #[tokio::test]
    async fn a_failed_invite_claim_answers_500() {
        let response = callback_with_store(Some("claim_invite"), false).await;
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_json(response).await["error"]["type"], "user_store_error");
    }

    #[tokio::test]
    async fn a_failed_invite_membership_answers_provisioning_failure() {
        let response = callback_with_store(Some("add_membership"), true).await;
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_json(response).await["error"]["type"], "provisioning_failed");
    }
