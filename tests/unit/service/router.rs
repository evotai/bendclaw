use axum::body::Body;
use axum::http::Request;
use bendclaw::config::AuthConfig;
use bendclaw::service::router::api_router;
use bendclaw::service::state::AppState;
use tower::ServiceExt;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn test_app_state(auth_key: &str) -> AppState {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    AppState {
        runtime: test_runtime(fake),
        auth_key: auth_key.to_string(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
    }
}

#[tokio::test]
async fn health_route_bypasses_auth() {
    let auth = AuthConfig {
        api_key: "secret".to_string(),
        cors_origins: Vec::new(),
    };
    let router = api_router(test_app_state("secret"), "info", &auth);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("health response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn protected_route_still_requires_auth() {
    let auth = AuthConfig {
        api_key: "secret".to_string(),
        cors_origins: Vec::new(),
    };
    let router = api_router(test_app_state("secret"), "info", &auth);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("protected response");

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}
