//! Mock GitHub OAuth server (ITEM-036/039 smoke scenario). Runs inside the
//! same binary (`inferon --mock-github`) so the deployment unit stays one
//! image; the smoke stack starts it under the `mock` compose profile and
//! points the proxy's OAuth env at it:
//!
//!   - `GET /login/oauth/authorize?redirect_uri=…&state=…[&login=…]` → 302 to
//!     `redirect_uri?code=…&state=<echoed state>` (the browser hop);
//!   - `POST /login/oauth/access_token` → `{"access_token": …}`;
//!   - `GET /user` (Bearer …) → the identity for that token.
//!
//! Identities: the default sign-in resolves to one fixed developer (smoke-dev);
//! passing `login=<name>` on the authorize hop encodes the login in the code,
//! so the smoke can sign in different users (ITEM-039 members claim invites).

use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

const DEFAULT_LOGIN: &str = "smoke-dev";
const DEFAULT_ID: i64 = 555_0001;

#[derive(Deserialize)]
struct AuthorizeParams {
    redirect_uri: String,
    state: Option<String>,
    login: Option<String>,
}

/// The code an authorize hop issues: encodes the requested login so the token
/// exchange can recover the identity without server-side state.
fn code_for(login: Option<&str>) -> String {
    match login {
        None => "mock-code".to_string(),
        Some(login) if login.is_empty() || login == DEFAULT_LOGIN => "mock-code".to_string(),
        Some(login) => format!("mock-code-{}", login.to_ascii_lowercase()),
    }
}

async fn authorize(Query(params): Query<AuthorizeParams>) -> Response {
    let state = params.state.unwrap_or_default();
    let location = format!(
        "{}?code={}&state={}",
        params.redirect_uri,
        code_for(params.login.as_deref()),
        crate::auth::encode_query(&state)
    );
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

fn login_for_code(code: &str) -> String {
    code.strip_prefix("mock-code-")
        .filter(|s| !s.is_empty() && *s != "mock-code")
        .unwrap_or(DEFAULT_LOGIN)
        .to_string()
}

async fn access_token(body: String) -> Response {
    let code = body
        .split('&')
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == "code").then_some(v.to_string())
        })
        .unwrap_or_else(|| "mock-code".to_string());
    Json(json!({ "access_token": format!("mock-token-{}", login_for_code(&code)) }))
        .into_response()
}

fn identity_for_login(login: &str) -> Value {
    let id = match login {
        DEFAULT_LOGIN => DEFAULT_ID,
        "smoke-other" => 555_0002,
        _ => 555_0003,
    };
    json!({ "id": id, "login": login })
}

async fn user(headers: axum::http::HeaderMap) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let login = token
        .strip_prefix("mock-token-")
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LOGIN)
        .to_string();
    Json(identity_for_login(&login)).into_response()
}

pub fn router() -> Router {
    Router::new()
        .route("/login/oauth/authorize", get(authorize))
        .route("/login/oauth/access_token", post(access_token))
        .route("/user", get(user))
}

pub async fn serve(port: u16) -> std::io::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router()).await
}

#[cfg(test)]
mod tests;
