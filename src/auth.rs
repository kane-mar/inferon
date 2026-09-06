//! Accounts: GitHub OAuth sign-in + signed-cookie sessions (ITEM-035).
//!
//! Humans get browser sessions; machines keep API keys (`/v1/chat` semantics
//! are untouched — sessions gate nothing yet, ITEM-035 AC4). This module owns:
//! the `users` store (upsert on GitHub id), the GitHub OAuth client seam, and
//! the `/auth/*` handlers. Session tokens/cookies live in `session.rs`.

use uuid::Uuid;

use axum::{
    extract::{FromRequestParts, Query, State},
    http::{
        header::{self, HeaderMap, HeaderValue},
        request::Parts,
        StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

/// A human account, keyed by GitHub id (migration 007).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub github_id: i64,
    pub github_login: String,
}

/// Lookup/upsert seam over `users`. InMemoryUserStore backs unit tests;
/// PostgresUserStore is the production store (schema 007: users keyed by a
/// unique github_id — sign-in upserts, so repeated logins keep one row).
#[async_trait::async_trait]
pub trait UserStore: Send + Sync {
    /// Record a successful GitHub sign-in: insert on first login, refresh the
    /// login/last_login on later ones. Returns the canonical user row.
    async fn upsert_github_user(
        &self,
        github_id: i64,
        github_login: &str,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>>;

    /// Resolve a user by id (session cookie carries the id). `None` when the
    /// row is gone — treated as signed out, never an error.
    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Clone, Default)]
pub struct InMemoryUserStore {
    users: std::sync::Arc<std::sync::Mutex<Vec<User>>>,
}

impl InMemoryUserStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<User>> {
        self.users.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait::async_trait]
impl UserStore for InMemoryUserStore {
    async fn upsert_github_user(
        &self,
        github_id: i64,
        github_login: &str,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        let mut users = self.lock();
        if let Some(existing) = users.iter_mut().find(|u| u.github_id == github_id) {
            existing.github_login = github_login.to_string();
            return Ok(existing.clone());
        }
        let user = User {
            id: Uuid::new_v4(),
            github_id,
            github_login: github_login.to_string(),
        };
        users.push(user.clone());
        Ok(user)
    }

    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        let users = self.lock();
        Ok(users.iter().find(|u| u.id == user_id).cloned())
    }
}

/// Production users store over Postgres (migration 007).
pub struct PostgresUserStore {
    pool: sqlx::PgPool,
}

impl PostgresUserStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserStore for PostgresUserStore {
    async fn upsert_github_user(
        &self,
        github_id: i64,
        github_login: &str,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, i64, String)>(
            "INSERT INTO users (github_id, github_login)
             VALUES ($1, $2)
             ON CONFLICT (github_id) DO UPDATE
               SET github_login = EXCLUDED.github_login, last_login_at = NOW()
             RETURNING id, github_id, github_login",
        )
        .bind(github_id)
        .bind(github_login)
        .fetch_one(&self.pool)
        .await?;
        Ok(User {
            id: row.0,
            github_id: row.1,
            github_login: row.2,
        })
    }

    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, i64, String)>(
            "SELECT id, github_id, github_login FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, github_id, github_login)| User {
            id,
            github_id,
            github_login,
        }))
    }
}

/// OAuth credentials (env: GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET). Endpoints
/// are GitHub constants — no env surface for them (YAGNI; tests never hit the
/// network, they inject a fake `GithubClient`).
#[derive(Clone, Debug)]
pub struct OauthConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// A GitHub identity returned by an OAuth code exchange.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubUser {
    pub github_id: i64,
    pub login: String,
}

/// Code-exchange seam. `RealGithub` talks to GitHub via reqwest; tests inject
/// a fake so flows never touch the network (ITEM-035 AC3).
#[async_trait::async_trait]
pub trait GithubClient: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<GithubUser, String>;
}

/// Real GitHub OAuth client: exchanges an authorization code for an access
/// token, then fetches the authenticated user (`github_id` + login).
/// https://docs.github.com/en/apps/oauth-apps/building-oauth-apps
pub struct RealGithub {
    http: reqwest::Client,
    config: OauthConfig,
}

impl RealGithub {
    pub fn new(http: reqwest::Client, config: OauthConfig) -> Self {
        Self { http, config }
    }
}

#[async_trait::async_trait]
impl GithubClient for RealGithub {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<GithubUser, String> {
        let token_resp = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;
        let token_json: serde_json::Value = token_resp
            .json()
            .await
            .map_err(|e| format!("token response unreadable: {e}"))?;
        let access_token = token_json["access_token"]
            .as_str()
            .ok_or_else(|| "no access_token in the exchange response".to_string())?;
        let user_resp = self
            .http
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "inferon")
            .send()
            .await
            .map_err(|e| format!("user request failed: {e}"))?;
        let user_json: serde_json::Value = user_resp
            .json()
            .await
            .map_err(|e| format!("user response unreadable: {e}"))?;
        let github_id = user_json["id"]
            .as_i64()
            .ok_or_else(|| "user response lacks a numeric id".to_string())?;
        let login = user_json["login"]
            .as_str()
            .ok_or_else(|| "user response lacks a login".to_string())?
            .to_string();
        Ok(GithubUser { github_id, login })
    }
}

/// Everything a session-capable deployment needs, resolved once from env in
/// `main.rs`. `AppState.accounts = None` when any required env var is missing
/// (ITEM-035 AC3: `/auth/login` answers 503 then).
pub struct Accounts {
    pub oauth: OauthConfig,
    pub session_secret: String,
    pub users: std::sync::Arc<dyn UserStore>,
    pub github: std::sync::Arc<dyn GithubClient>,
}

impl Accounts {
    /// `Some` only when GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET and
    /// SESSION_SECRET are all set — a partially configured deployment is
    /// treated as unconfigured (fail closed on the login screen).
    pub fn from_env(
        http: reqwest::Client,
        users: std::sync::Arc<dyn UserStore>,
    ) -> Option<std::sync::Arc<Accounts>> {
        let client_id = std::env::var("GITHUB_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GITHUB_CLIENT_SECRET").ok()?;
        let session_secret = std::env::var("SESSION_SECRET").ok()?;
        if client_id.is_empty() || client_secret.is_empty() || session_secret.is_empty() {
            return None;
        }
        let oauth = OauthConfig {
            client_id,
            client_secret,
        };
        Some(std::sync::Arc::new(Accounts {
            oauth: oauth.clone(),
            session_secret,
            users,
            github: std::sync::Arc::new(RealGithub::new(http, oauth)),
        }))
    }
}

/// GitHub's OAuth authorize endpoint URL for a sign-in attempt. `state` is the
/// CSRF nonce; `redirect_uri` is where GitHub sends the code back.
pub fn authorize_url(config: &OauthConfig, redirect_uri: &str, state: &str) -> String {
    format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&state={}&scope=read:user",
        encode_query(&config.client_id),
        encode_query(redirect_uri),
        encode_query(state)
    )
}

/// Percent-encode a value for a query string, keeping RFC 3986 unreserved
/// characters only.
pub fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

// --- /auth/* handlers (ITEM-035) ---
//
// Sessions gate nothing yet (AC4): /v1/chat stays machine-key-authed and the
// dashboard/operator views keep their bearer gates. These routes only create
// and read a human session; ITEM-036/037 attach gates on top.

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// True when the request arrived over TLS. Caddy (the TLS edge) sets
/// `X-Forwarded-Proto: https`; its absence means a plain-http hop (local dev
/// or a misconfigured edge) and cookies drop the `Secure` attribute.
fn is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|p| p.split(',').next().unwrap_or("").trim() == "https")
        .unwrap_or(false)
}

fn host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost")
        .to_string()
}

/// The OAuth redirect URI for this request: GitHub must send the code back to
/// the same origin that started the dance.
fn redirect_uri(headers: &HeaderMap) -> String {
    let scheme = if is_secure(headers) { "https" } else { "http" };
    format!("{scheme}://{}/auth/callback", host(headers))
}

fn add_cookie(response: &mut Response, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        response.headers_mut().append(header::SET_COOKIE, v);
    }
}

fn oauth_unconfigured() -> Response {
    crate::json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "oauth_not_configured",
        "GitHub OAuth is not configured on this server",
    )
}

fn signed_out() -> Response {
    crate::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "not signed in")
}

fn redirect_to(location: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, location.to_string())]).into_response()
}

/// GET /auth/login — start the GitHub OAuth dance (AC1). Answers 503 when
/// OAuth is unconfigured (AC3). Sets a short-lived CSRF `state` cookie; the
/// browser follows the redirect to GitHub and comes back via /auth/callback.
pub async fn login(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(accounts) = &state.accounts else {
        return oauth_unconfigured();
    };
    let nonce = Uuid::new_v4().simple().to_string();
    let uri = redirect_uri(&headers);
    let url = authorize_url(&accounts.oauth, &uri, &nonce);
    let mut response = redirect_to(&url);
    add_cookie(
        &mut response,
        &crate::session::cookie(
            crate::session::OAUTH_STATE_COOKIE,
            &nonce,
            crate::session::OAUTH_STATE_TTL_SECS,
            is_secure(&headers),
        ),
    );
    response
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// GET /auth/callback?code=…&state=… — GitHub redirected the browser back.
/// Verifies the CSRF state against the cookie, exchanges the code for a
/// GitHub identity, upserts the user, and sets the signed session cookie
/// (AC1/AC2). State mismatches and failed exchanges answer 4xx/5xx without a
/// session; the one-time state cookie is cleared on every outcome.
pub async fn callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(accounts) = &state.accounts else {
        return oauth_unconfigured();
    };
    let secure = is_secure(&headers);
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // CSRF: the state query param must equal the one-time state cookie.
    let state_matches =
        match (query.state.as_deref(), crate::session::cookie_value(cookie_header, crate::session::OAUTH_STATE_COOKIE)) {
            (Some(state), Some(expected)) => state == expected,
            _ => false,
        };

    let outcome = match (query.code.as_deref(), state_matches) {
        (None, _) => crate::json_error(
            StatusCode::BAD_REQUEST,
            "oauth_callback_error",
            "GitHub did not return an authorization code",
        ),
        (Some(_), false) => crate::json_error(
            StatusCode::BAD_REQUEST,
            "oauth_state_mismatch",
            "OAuth state mismatch — start the sign-in again",
        ),
        (Some(code), true) => match accounts
            .github
            .exchange_code(code, &redirect_uri(&headers))
            .await
        {
            Err(e) => {
                tracing::warn!("GitHub OAuth exchange failed: {e}");
                crate::json_error(
                    StatusCode::BAD_GATEWAY,
                    "github_exchange_failed",
                    "GitHub did not complete the sign-in",
                )
            }
            Ok(github_user) => match accounts
                .users
                .upsert_github_user(github_user.github_id, &github_user.login)
                .await
            {
                Err(e) => {
                    tracing::error!("user upsert failed: {e}");
                    crate::json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "user_store_error",
                        "could not record the sign-in",
                    )
                }
                Ok(user) => {
                    let token = crate::session::issue_session(
                        user.id,
                        &accounts.session_secret,
                        now_secs(),
                        crate::session::SESSION_TTL_SECS,
                    );
                    let mut response = redirect_to("/v1/dashboard");
                    add_cookie(&mut response, &crate::session::set_cookie(&token, secure));
                    response
                }
            },
        },
    };
    // The state cookie is one-time: clear it on every outcome.
    let mut response = outcome;
    add_cookie(
        &mut response,
        &crate::session::cookie(crate::session::OAUTH_STATE_COOKIE, "", 0, secure),
    );
    response
}

/// POST /auth/logout — expire the session cookie and land on the dashboard
/// (AC2). Works even when OAuth is unconfigured.
pub async fn logout(headers: HeaderMap) -> Response {
    let mut response = redirect_to("/v1/dashboard");
    add_cookie(&mut response, &crate::session::clear_cookie(is_secure(&headers)));
    response
}

/// The signed-in user, resolved from the session cookie by `me` (AC4). A
/// missing/forged/expired cookie rejects with 401 — never an error.
pub struct CurrentUser(pub User);

#[async_trait::async_trait]
impl FromRequestParts<Arc<AppState>> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(accounts) = &state.accounts else {
            return Err(signed_out());
        };
        let cookie = parts
            .headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let Some(token) = crate::session::cookie_value(cookie, crate::session::SESSION_COOKIE) else {
            return Err(signed_out());
        };
        let Some(user_id) = crate::session::verify_session(token, &accounts.session_secret, now_secs())
        else {
            return Err(signed_out());
        };
        match accounts.users.find_by_id(user_id).await {
            Ok(Some(user)) => Ok(CurrentUser(user)),
            Ok(None) => Err(signed_out()),
            Err(e) => {
                tracing::error!("user lookup failed: {e}");
                Err(crate::json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user_store_error",
                    "could not resolve the session",
                ))
            }
        }
    }
}

/// GET /auth/me — the signed-in user, or 401 (AC4).
pub async fn me(CurrentUser(user): CurrentUser) -> Response {
    Json(json!({
        "id": user.id,
        "github_id": user.github_id,
        "github_login": user.github_login,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> InMemoryUserStore {
        InMemoryUserStore::default()
    }

    #[tokio::test]
    async fn first_sign_in_creates_a_user_row() {
        let store = store();
        let user = store
            .upsert_github_user(42, "kane")
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
        let first = store.upsert_github_user(42, "kane").await.expect("store ok");
        let second = store.upsert_github_user(42, "KaneMar").await.expect("store ok");
        assert_eq!(second.id, first.id, "same GitHub id keeps the same user row");
        assert_eq!(second.github_login, "KaneMar");
        // Still exactly one row for that GitHub id.
        let found = store.find_by_id(first.id).await.expect("store ok");
        assert_eq!(found.map(|u| u.github_login), Some("KaneMar".to_string()));
    }

    #[tokio::test]
    async fn different_github_ids_are_distinct_users() {
        let store = store();
        let a = store.upsert_github_user(1, "alice").await.expect("store ok");
        let b = store.upsert_github_user(2, "bob").await.expect("store ok");
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

    // --- ITEM-035 AC1/AC3: OAuth authorize URL + query encoding ---

    fn config() -> OauthConfig {
        OauthConfig {
            client_id: "client-123".to_string(),
            client_secret: "secret".to_string(),
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
            },
            session_secret: SECRET.to_string(),
            users,
            github,
        }))
    }

    fn test_state(accounts: Option<std::sync::Arc<Accounts>>) -> std::sync::Arc<crate::AppState> {
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        std::sync::Arc::new(crate::AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/".to_string(),
            tenant_store: std::sync::Arc::new(crate::InMemoryTenantStore::default()),
            budget_store: std::sync::Arc::new(crate::budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        })
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

    #[tokio::test]
    async fn callback_establishes_a_session_and_upserts_the_user() {
        let users = std::sync::Arc::new(InMemoryUserStore::default());
        let github = std::sync::Arc::new(FakeGithub::default());
        let state = test_state(test_accounts(users.clone(), github.clone()));
        let router = crate::build_router(state);

        // Login -> capture the state nonce and cookie.
        let login = router
            .clone()
            .oneshot(request("/auth/login", "GET", &[], false))
            .await
            .unwrap();
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

        // Callback with the right code + state.
        let callback = router
            .clone()
            .oneshot(request(
                &format!("/auth/callback?code=the-code&state={nonce}"),
                "GET",
                &[(crate::session::OAUTH_STATE_COOKIE, &state_cookie)],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(callback.status(), axum::http::StatusCode::FOUND, "lands on the dashboard");
        assert_eq!(callback.headers()[header::LOCATION], "/v1/dashboard");
        let cookies = set_cookies(&callback);
        let token = cookies
            .get(crate::session::SESSION_COOKIE)
            .expect("session cookie set")
            .to_string();
        // The one-time state cookie is cleared (present with an empty value).
        assert_eq!(
            cookies.get(crate::session::OAUTH_STATE_COOKIE).map(String::as_str),
            Some(""),
            "state cookie cleared"
        );

        // The session token resolves to the upserted user.
        let user_id =
            crate::session::verify_session(&token, SECRET, now_secs()).expect("fresh token valid");
        let user = users.find_by_id(user_id).await.expect("store ok").expect("user present");
        assert_eq!((user.github_id, user.github_login.as_str()), (42, "octocat"));

        // The exchange saw the code and the http redirect URI (plain-http test hop).
        let recorded = github.calls.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![("the-code".to_string(), "http://inferon.example/auth/callback".to_string())]
        );

        // /auth/me with the session cookie returns the user.
        let me = router
            .clone()
            .oneshot(request(
                "/auth/me",
                "GET",
                &[(crate::session::SESSION_COOKIE, &token)],
                false,
            ))
            .await
            .unwrap();
        assert_eq!(me.status(), axum::http::StatusCode::OK);
        let json = body_json(me).await;
        assert_eq!(json["github_id"], 42);
        assert_eq!(json["github_login"], "octocat");
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
            set_cookies(&callback)
                .get(crate::session::SESSION_COOKIE)
                .is_none(),
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
        assert_eq!(response.headers()[header::LOCATION], "/v1/dashboard");
        let raw = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with("inferon_session="))
            .expect("session clear cookie");
        assert!(raw.contains("Max-Age=0"), "expired: {raw}");
    }
}
