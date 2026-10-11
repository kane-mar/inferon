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

/// A human account, keyed by GitHub id (migration 007). `is_operator` flags a
/// platform operator (ITEM-037, migration 009) — their session scopes to the
/// platform-wide operator views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub github_id: i64,
    pub github_login: String,
    pub is_operator: bool,
}

/// Lookup/upsert seam over `users`. InMemoryUserStore backs unit tests;
/// PostgresUserStore is the production store (schema 007: users keyed by a
/// unique github_id — sign-in upserts, so repeated logins keep one row).
#[async_trait::async_trait]
pub trait UserStore: Send + Sync {
    /// Record a successful GitHub sign-in: insert on first login, refresh the
    /// login/last_login on later ones. Returns the canonical user row.
    /// `is_operator` is the env-authoritative operator flag (ITEM-037: the
    /// user's login is in OPERATOR_GITHUB_LOGINS).
    async fn upsert_github_user(
        &self,
        github_id: i64,
        github_login: &str,
        is_operator: bool,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>>;

    /// Mark every existing user whose github_login is in `logins` as an
    /// operator (boot backfill from OPERATOR_GITHUB_LOGINS, ITEM-037 AC1).
    /// Returns how many rows were actually promoted.
    async fn promote_logins(
        &self,
        logins: &[&str],
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>;

    /// Resolve a user by id (session cookie carries the id). `None` when the
    /// row is gone — treated as signed out, never an error.
    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>>;

    /// The user's tenant membership (tenant_id, role). `None` when the user
    /// belongs to no tenant (ITEM-036: such a user gets auto-provisioned on
    /// their next sign-in).
    async fn tenant_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(Uuid, String)>, Box<dyn std::error::Error + Send + Sync>>;

    /// Record a user's membership of a tenant (migration 008). Idempotent for
    /// an existing (tenant, user) pair.
    async fn add_membership(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Invite a GitHub login to join a tenant as `member` (ITEM-039,
    /// migration 010). The invite is claimed when that user signs in.
    async fn invite_member(
        &self,
        tenant_id: Uuid,
        github_login: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Claim any pending invite for `github_login`, consuming it. Returns the
    /// tenant the user should join, if invited.
    async fn claim_invite(
        &self,
        github_login: &str,
    ) -> Result<Option<Uuid>, Box<dyn std::error::Error + Send + Sync>>;

    /// The tenant's members (user id, github login, role), oldest first.
    async fn list_members(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<(Uuid, String, String)>, Box<dyn std::error::Error + Send + Sync>>;

    /// Remove a member (and any pending invite under their login) from a
    /// tenant. True when something was actually removed. Never removes the
    /// tenant's owner.
    async fn remove_member(
        &self,
        tenant_id: Uuid,
        member_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    /// Pending invites for a tenant (github logins, oldest first).
    async fn list_invites(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Clone, Default)]
pub struct InMemoryUserStore {
    users: std::sync::Arc<std::sync::Mutex<Vec<User>>>,
    memberships: std::sync::Arc<std::sync::Mutex<Vec<(Uuid, Uuid, String)>>>, // (tenant, user, role)
    invites: std::sync::Arc<std::sync::Mutex<Vec<(Uuid, String)>>>, // (tenant, github_login)
}

impl InMemoryUserStore {
    fn lock_users(&self) -> std::sync::MutexGuard<'_, Vec<User>> {
        self.users.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_memberships(&self) -> std::sync::MutexGuard<'_, Vec<(Uuid, Uuid, String)>> {
        self.memberships.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_invites(&self) -> std::sync::MutexGuard<'_, Vec<(Uuid, String)>> {
        self.invites.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait::async_trait]
impl UserStore for InMemoryUserStore {
    async fn upsert_github_user(
        &self,
        github_id: i64,
        github_login: &str,
        is_operator: bool,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        let mut users = self.lock_users();
        if let Some(existing) = users.iter_mut().find(|u| u.github_id == github_id) {
            existing.github_login = github_login.to_string();
            existing.is_operator = is_operator;
            return Ok(existing.clone());
        }
        let user = User {
            id: Uuid::new_v4(),
            github_id,
            github_login: github_login.to_string(),
            is_operator,
        };
        users.push(user.clone());
        Ok(user)
    }

    async fn promote_logins(
        &self,
        logins: &[&str],
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut users = self.lock_users();
        let mut promoted = 0;
        for user in users.iter_mut() {
            if !user.is_operator && logins.contains(&user.github_login.as_str()) {
                user.is_operator = true;
                promoted += 1;
            }
        }
        Ok(promoted)
    }

    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        let users = self.lock_users();
        Ok(users.iter().find(|u| u.id == user_id).cloned())
    }

    async fn tenant_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(Uuid, String)>, Box<dyn std::error::Error + Send + Sync>> {
        let memberships = self.lock_memberships();
        Ok(memberships
            .iter()
            .find(|(_, uid, _)| *uid == user_id)
            .map(|(tid, _, role)| (*tid, role.clone())))
    }

    async fn add_membership(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut memberships = self.lock_memberships();
        if memberships.iter().any(|(tid, uid, _)| *tid == tenant_id && *uid == user_id) {
            return Ok(());
        }
        memberships.push((tenant_id, user_id, role.to_string()));
        Ok(())
    }

    async fn invite_member(
        &self,
        tenant_id: Uuid,
        github_login: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut invites = self.lock_invites();
        if !invites.iter().any(|(tid, login)| *tid == tenant_id && login == github_login) {
            invites.push((tenant_id, github_login.to_string()));
        }
        Ok(())
    }

    async fn claim_invite(
        &self,
        github_login: &str,
    ) -> Result<Option<Uuid>, Box<dyn std::error::Error + Send + Sync>> {
        let mut invites = self.lock_invites();
        let index = invites.iter().position(|(_, login)| login == github_login);
        Ok(index.map(|i| invites.remove(i).0))
    }

    async fn list_members(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<(Uuid, String, String)>, Box<dyn std::error::Error + Send + Sync>> {
        let memberships = self.lock_memberships();
        let users = self.lock_users();
        let rows: Vec<(Uuid, String, String)> = memberships
            .iter()
            .filter(|(tid, _, _)| *tid == tenant_id)
            .map(|(_, uid, role)| {
                let login = users
                    .iter()
                    .find(|u| u.id == *uid)
                    .map(|u| u.github_login.clone())
                    .unwrap_or_default();
                (*uid, login, role.clone())
            })
            .collect();
        Ok(rows)
    }

    async fn remove_member(
        &self,
        tenant_id: Uuid,
        member_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let mut memberships = self.lock_memberships();
        let Some(index) = memberships
            .iter()
            .position(|(tid, uid, role)| *tid == tenant_id && *uid == member_id && role != "owner")
        else {
            return Ok(false);
        };
        memberships.remove(index);
        // Drop any pending invite under the removed member's login.
        let login = self
            .lock_users()
            .iter()
            .find(|u| u.id == member_id)
            .map(|u| u.github_login.clone());
        if let Some(login) = login {
            let mut invites = self.lock_invites();
            invites.retain(|(tid, l)| !(*tid == tenant_id && *l == login));
        }
        Ok(true)
    }

    async fn list_invites(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let invites = self.lock_invites();
        Ok(invites
            .iter()
            .filter(|(tid, _)| *tid == tenant_id)
            .map(|(_, login)| login.clone())
            .collect())
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
        is_operator: bool,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, i64, String, bool)>(
            "INSERT INTO users (github_id, github_login, is_operator)
             VALUES ($1, $2, $3)
             ON CONFLICT (github_id) DO UPDATE
               SET github_login = EXCLUDED.github_login,
                   is_operator = EXCLUDED.is_operator,
                   last_login_at = NOW()
             RETURNING id, github_id, github_login, is_operator",
        )
        .bind(github_id)
        .bind(github_login)
        .bind(is_operator)
        .fetch_one(&self.pool)
        .await?;
        Ok(User {
            id: row.0,
            github_id: row.1,
            github_login: row.2,
            is_operator: row.3,
        })
    }

    async fn promote_logins(
        &self,
        logins: &[&str],
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let result = sqlx::query(
            "UPDATE users SET is_operator = true
             WHERE github_login = ANY($1) AND NOT is_operator",
        )
        .bind(logins)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, i64, String, bool)>(
            "SELECT id, github_id, github_login, is_operator FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, github_id, github_login, is_operator)| User {
            id,
            github_id,
            github_login,
            is_operator,
        }))
    }

    async fn tenant_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(Uuid, String)>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT tenant_id, role FROM tenant_members WHERE user_id = $1 ORDER BY created_at LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn add_membership(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO tenant_members (tenant_id, user_id, role)
             VALUES ($1, $2, $3)
             ON CONFLICT (tenant_id, user_id) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn invite_member(
        &self,
        tenant_id: Uuid,
        github_login: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO tenant_invites (tenant_id, github_login)
             VALUES ($1, $2)
             ON CONFLICT (tenant_id, github_login) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(github_login)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_invite(
        &self,
        github_login: &str,
    ) -> Result<Option<Uuid>, Box<dyn std::error::Error + Send + Sync>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "DELETE FROM tenant_invites WHERE github_login = $1 RETURNING tenant_id",
        )
        .bind(github_login)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn list_members(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<(Uuid, String, String)>, Box<dyn std::error::Error + Send + Sync>> {
        let rows = sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT u.id, u.github_login, tm.role
             FROM tenant_members tm JOIN users u ON u.id = tm.user_id
             WHERE tm.tenant_id = $1 ORDER BY tm.created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn remove_member(
        &self,
        tenant_id: Uuid,
        member_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let removed = sqlx::query(
            "DELETE FROM tenant_members
             WHERE tenant_id = $1 AND user_id = $2 AND role = 'member'",
        )
        .bind(tenant_id)
        .bind(member_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;
        if removed {
            // Drop any pending invite under the removed member's login.
            sqlx::query(
                "DELETE FROM tenant_invites
                 WHERE tenant_id = $1 AND github_login =
                       (SELECT github_login FROM users WHERE id = $2)",
            )
            .bind(tenant_id)
            .bind(member_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(removed)
    }

    async fn list_invites(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT github_login FROM tenant_invites
             WHERE tenant_id = $1 ORDER BY created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}

/// OAuth endpoints + credentials. Credentials come from env
/// (GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET); the three endpoint URLs default
/// to GitHub and can be overridden (GITHUB_AUTHORIZE_URL / GITHUB_TOKEN_URL /
/// GITHUB_API_URL) so the smoke can point the real client at the in-binary
/// mock GitHub (ITEM-036 AC4).
#[derive(Clone, Debug)]
pub struct OauthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub api_url: String,
}

impl OauthConfig {
    fn env(key: &str, fallback: &str) -> String {
        std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback.to_string())
    }
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
            .post(&self.config.token_url)
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
            .get(format!("{}/user", self.config.api_url))
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
    /// GitHub logins granted the platform-operator role (env
    /// `OPERATOR_GITHUB_LOGINS`, comma-separated; ITEM-037 AC1). Applied at
    /// sign-in and backfilled to existing users at boot.
    pub operator_logins: std::collections::HashSet<String>,
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
        let operator_logins = std::env::var("OPERATOR_GITHUB_LOGINS")
            .ok()
            .map(|list| {
                list.split(',')
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let oauth = OauthConfig {
            client_id,
            client_secret,
            authorize_url: OauthConfig::env(
                "GITHUB_AUTHORIZE_URL",
                "https://github.com/login/oauth/authorize",
            ),
            token_url: OauthConfig::env(
                "GITHUB_TOKEN_URL",
                "https://github.com/login/oauth/access_token",
            ),
            api_url: OauthConfig::env("GITHUB_API_URL", "https://api.github.com"),
        };
        Some(std::sync::Arc::new(Accounts {
            oauth: oauth.clone(),
            session_secret,
            operator_logins,
            users,
            github: std::sync::Arc::new(RealGithub::new(http, oauth)),
        }))
    }
}

/// GitHub's OAuth authorize endpoint URL for a sign-in attempt. `state` is the
/// CSRF nonce; `redirect_uri` is where GitHub sends the code back.
pub fn authorize_url(config: &OauthConfig, redirect_uri: &str, state: &str) -> String {
    format!(
        "{}?client_id={}&redirect_uri={}&state={}&scope=read:user",
        config.authorize_url,
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
                .upsert_github_user(
                    github_user.github_id,
                    &github_user.login,
                    accounts.operator_logins.contains(&github_user.login),
                )
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
                    let session_cookie = crate::session::set_cookie(&token, secure);
                    match accounts.users.tenant_for_user(user.id).await {
                        Err(e) => {
                            tracing::error!("membership lookup failed: {e}");
                            crate::json_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "user_store_error",
                                "could not resolve the sign-in",
                            )
                        }
                        // Returning user: straight to their dashboard (ITEM-036 AC2).
                        Ok(Some(_)) => {
                            let mut response = redirect_to("/dashboard");
                            add_cookie(&mut response, &session_cookie);
                            response
                        }
                        // A first-time user either claims a member invite
                        // (ITEM-039: join the inviter's tenant, no key) or
                        // auto-provisions a tenant + owner + key (ITEM-036).
                        Ok(None) => match accounts.users.claim_invite(&user.github_login).await {
                            Err(e) => {
                                tracing::error!("invite claim failed: {e}");
                                crate::json_error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "user_store_error",
                                    "could not check your invite",
                                )
                            }
                            Ok(Some(invited_tenant)) => {
                                match accounts
                                    .users
                                    .add_membership(invited_tenant, user.id, "member")
                                    .await
                                {
                                    Err(e) => {
                                        tracing::error!("membership insert failed: {e}");
                                        crate::json_error(
                                            StatusCode::INTERNAL_SERVER_ERROR,
                                            "provisioning_failed",
                                            "could not join your team's tenant",
                                        )
                                    }
                                    Ok(()) => {
                                        let mut response = redirect_to("/dashboard");
                                        add_cookie(&mut response, &session_cookie);
                                        response
                                    }
                                }
                            }
                            Ok(None) => {
                                match provision_first_tenant(&state, accounts, user).await {
                                    Err(e) => e,
                                    // No key is minted here: the frontend's
                                    // keys page mints the first one (shown
                                    // once) — the backend renders no HTML.
                                    Ok(()) => {
                                        let mut response = redirect_to("/dashboard/onboarding");
                                        add_cookie(&mut response, &session_cookie);
                                        response
                                    }
                                }
                            }
                        },
                    }
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
    let mut response = redirect_to("/dashboard");
    add_cookie(&mut response, &crate::session::clear_cookie(is_secure(&headers)));
    response
}

fn provisioning_failed() -> Response {
    crate::json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "provisioning_failed",
        "could not set up your tenant — please try signing in again",
    )
}

/// ITEM-036 AC1: create the tenant and record the user as its owner. The first
/// API key is minted by the frontend's keys page (and shown once there) — the
/// backend renders no HTML.
async fn provision_first_tenant(
    state: &Arc<AppState>,
    accounts: &Accounts,
    user: User,
) -> Result<(), Response> {
    let tenant_id = match state
        .tenant_store
        .insert_tenant(None)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("tenant insert failed: {e}");
            return Err(provisioning_failed());
        }
    };
    if let Err(e) = accounts.users.add_membership(tenant_id, user.id, "owner").await {
        tracing::error!("membership insert failed: {e}");
        return Err(provisioning_failed());
    }
    Ok(())
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
        "is_operator": user.is_operator,
    }))
    .into_response()
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;

    /// Session secret shared by test-only account fixtures.
    pub(crate) const TEST_SESSION_SECRET: &str = "test-session-secret";

    struct NeverGithub;

    #[async_trait::async_trait]
    impl GithubClient for NeverGithub {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
        ) -> Result<GithubUser, String> {
            Err("no network in unit tests".to_string())
        }
    }

    /// An OAuth-configured `Accounts` over the given user store (dashboard
    /// session-scope tests, ITEM-036 AC2).
    pub(crate) fn dummy_accounts(
        users: std::sync::Arc<dyn UserStore>,
    ) -> std::sync::Arc<Accounts> {
        std::sync::Arc::new(Accounts {
            oauth: OauthConfig {
                client_id: "client-123".to_string(),
                client_secret: "secret".to_string(),
                authorize_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                api_url: "https://api.github.com".to_string(),
            },
            session_secret: TEST_SESSION_SECRET.to_string(),
            operator_logins: std::collections::HashSet::new(),
            users,
            github: std::sync::Arc::new(NeverGithub),
        })
    }
}

#[cfg(test)]
mod tests {
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
}
