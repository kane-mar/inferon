// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Accounts: GitHub OAuth sign-in + signed-cookie sessions (ITEM-035).
//!
//! Humans get browser sessions; machines keep API keys (`/v1/chat` semantics
//! are untouched — sessions gate nothing yet, ITEM-035 AC4). This module owns:
//! the `users` store (upsert on GitHub id), the GitHub OAuth client seam, and
//! the `/auth/*` handlers. Session tokens/cookies live in `session.rs`.

use crate::clock::now_secs;
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
pub(crate) fn authorize_url(config: &OauthConfig, redirect_uri: &str, state: &str) -> String {
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

    let outcome = match query.code.as_deref() {
        None => crate::json_error(
            StatusCode::BAD_REQUEST,
            "oauth_callback_error",
            "GitHub did not return an authorization code",
        ),
        Some(_) if !state_matches => crate::json_error(
            StatusCode::BAD_REQUEST,
            "oauth_state_mismatch",
            "OAuth state mismatch; start the sign-in again",
        ),
        Some(code) => complete_sign_in(&state, accounts, code, &headers, secure).await,
    };
    // The state cookie is one-time: clear it on every outcome.
    let mut response = outcome;
    add_cookie(
        &mut response,
        &crate::session::cookie(crate::session::OAUTH_STATE_COOKIE, "", 0, secure),
    );
    response
}

/// Exchange the code for a GitHub identity, record the sign-in, and route the
/// user: an existing member to their dashboard, a first-time user through
/// [`admit_user`]. Each failure is a typed JSON response.
async fn complete_sign_in(
    state: &Arc<AppState>,
    accounts: &Accounts,
    code: &str,
    headers: &HeaderMap,
    secure: bool,
) -> Response {
    let github_user = match accounts.github.exchange_code(code, &redirect_uri(headers)).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!("GitHub OAuth exchange failed: {e}");
            return crate::json_error(
                StatusCode::BAD_GATEWAY,
                "github_exchange_failed",
                "GitHub did not complete the sign-in",
            );
        }
    };
    let user = match accounts
        .users
        .upsert_github_user(
            github_user.github_id,
            &github_user.login,
            accounts.operator_logins.contains(&github_user.login),
        )
        .await
    {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("user upsert failed: {e}");
            return crate::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user_store_error",
                "could not record the sign-in",
            );
        }
    };
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
        Ok(Some(_)) => redirect_with_session("/dashboard", user.id, accounts, secure),
        Ok(None) => admit_user(state, accounts, user, secure).await,
    }
}

/// Admit a first-time user (no membership): join a pending invite as a member
/// (ITEM-039, no key), or auto-provision a tenant they own (ITEM-036). `user`
/// is consumed by provisioning.
async fn admit_user(
    state: &Arc<AppState>,
    accounts: &Accounts,
    user: User,
    secure: bool,
) -> Response {
    let user_id = user.id;
    match accounts.users.claim_invite(&user.github_login).await {
        Err(e) => {
            tracing::error!("invite claim failed: {e}");
            crate::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user_store_error",
                "could not check your invite",
            )
        }
        Ok(Some(invited_tenant)) => {
            match accounts.users.add_membership(invited_tenant, user_id, "member").await {
                Err(e) => {
                    tracing::error!("membership insert failed: {e}");
                    crate::json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "provisioning_failed",
                        "could not join your team's tenant",
                    )
                }
                Ok(()) => redirect_with_session("/dashboard", user_id, accounts, secure),
            }
        }
        // No key is minted here: the frontend's keys page mints the first one
        // (shown once) — the backend renders no HTML.
        Ok(None) => match provision_first_tenant(state, accounts, user).await {
            Err(resp) => resp,
            Ok(()) => redirect_with_session("/dashboard/onboarding", user_id, accounts, secure),
        },
    }
}

/// A redirect to `location` carrying a fresh session cookie for `user_id`.
fn redirect_with_session(
    location: &str,
    user_id: Uuid,
    accounts: &Accounts,
    secure: bool,
) -> Response {
    let token = crate::session::issue_session(
        user_id,
        &accounts.session_secret,
        now_secs(),
        crate::session::SESSION_TTL_SECS,
    );
    let mut response = redirect_to(location);
    add_cookie(&mut response, &crate::session::set_cookie(&token, secure));
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
        "could not set up your tenant; please try signing in again",
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
mod tests;
