//! Mock GitHub OAuth server (ITEM-036 AC4 smoke scenario). Runs inside the
//! same binary (`inferon --mock-github`) so the deployment unit stays one
//! image; the smoke stack starts it under the `mock` compose profile and
//! points the proxy's OAuth env at it:
//!
//!   - `GET /login/oauth/authorize?redirect_uri=…&state=…` → 302 to
//!     `redirect_uri?code=mock-code&state=<echoed state>` (the browser hop);
//!   - `POST /login/oauth/access_token` → `{"access_token": "mock-token"}`;
//!   - `GET /user` (Bearer mock-token) → one fixed identity the smoke signs up
//!     twice (fresh sign-in provisions, the repeat lands on the dashboard).

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

/// The fixed GitHub identity every mock sign-in resolves to.
const MOCK_GITHUB_ID: i64 = 555_0001;
const MOCK_LOGIN: &str = "smoke-dev";

#[derive(Deserialize)]
struct AuthorizeParams {
    redirect_uri: String,
    state: Option<String>,
}

async fn authorize(Query(params): Query<AuthorizeParams>) -> Response {
    let state = params.state.unwrap_or_default();
    let location = format!(
        "{}?code=mock-code&state={}",
        params.redirect_uri,
        crate::auth::encode_query(&state)
    );
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

async fn access_token() -> Json<Value> {
    Json(json!({ "access_token": "mock-token", "token_type": "bearer" }))
}

async fn user() -> Json<Value> {
    Json(json!({ "id": MOCK_GITHUB_ID, "login": MOCK_LOGIN }))
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
mod tests {
    use super::*;
    use tower::ServiceExt;

    async fn body(response: axum::response::Response) -> String {
        String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap().to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn authorize_redirects_back_with_a_code_and_the_state_echoed() {
        let response = router()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/login/oauth/authorize?redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fauth%2Fcallback&state=nonce123")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers()[header::LOCATION],
            "http://127.0.0.1:8080/auth/callback?code=mock-code&state=nonce123"
        );
    }

    #[tokio::test]
    async fn token_and_user_endpoints_return_the_mock_identity() {
        let router = router();
        let token = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/login/oauth/access_token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(body(token).await.contains("mock-token"));
        let user = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/user")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body(user).await;
        assert!(json.contains(&MOCK_GITHUB_ID.to_string()) && json.contains(MOCK_LOGIN));
    }
}
