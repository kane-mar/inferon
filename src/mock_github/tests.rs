    use super::*;
    use axum::body::Body;
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
                    .body(Body::empty())
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
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(body(token).await.contains("mock-token-smoke-dev"));
        let user = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/user")
                    .header("authorization", "Bearer mock-token-smoke-dev")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body(user).await;
        assert!(json.contains(&DEFAULT_ID.to_string()) && json.contains(DEFAULT_LOGIN));
    }

    #[tokio::test]
    async fn a_login_marker_encodes_a_second_identity_for_invited_members() {
        let router = router();
        // Authorize with login=smoke-other → a distinct code…
        let auth = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/login/oauth/authorize?redirect_uri=http%3A%2F%2Fh%2Fcallback&login=smoke-other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            auth.headers()[header::LOCATION],
            "http://h/callback?code=mock-code-smoke-other&state="
        );
        // …which exchanges for that user's identity.
        let token = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/login/oauth/access_token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from("code=mock-code-smoke-other"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let token_body = body(token).await;
        assert!(token_body.contains("mock-token-smoke-other"), "{token_body}");
        let user = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/user")
                    .header("authorization", "Bearer mock-token-smoke-other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body(user).await;
        assert!(json.contains("5550002") && json.contains("smoke-other"), "{json}");
    }
