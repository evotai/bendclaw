use axum::body::Body;
use axum::http::Request;
use bendclaw::config::AuthConfig;
use bendclaw::service::router::api_router;
use bendclaw::service::state::AppState;
use tower::ServiceExt;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn app_state(
    query_handler: impl Fn(&str, Option<&str>) -> bendclaw::types::Result<bendclaw::storage::pool::QueryResponse>
        + Send
        + Sync
        + 'static,
) -> AppState {
    let fake = FakeDatabend::new(query_handler);
    AppState {
        runtime: test_runtime(fake),
        auth_key: "test-key".to_string(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
    }
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        api_key: "test-key".to_string(),
        cors_origins: Vec::new(),
    }
}

fn replay_request(agent_id: &str, session_id: &str, user_id: &str) -> Request<Body> {
    Request::builder()
        .uri(format!(
            "/v1/agents/{agent_id}/workbench/sessions/{session_id}/replay"
        ))
        .header("authorization", "Bearer test-key")
        .header("x-user-id", user_id)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn replay_session_not_found_returns_404() {
    // Empty result for all queries → session_load returns None → 404
    let state = app_state(|_sql, _db| {
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
    let router = api_router(state, "info", &auth_config());

    let response = router
        .oneshot(replay_request("agent_1", "sess_nonexistent", "user_1"))
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn replay_session_wrong_user_returns_403() {
    // Return a session owned by a different user
    let state = app_state(|sql, _db| {
        if sql.contains("sessions") && sql.contains("WHERE") {
            // 12 columns matching SessionMapper: id, agent_id, user_id, title, scope,
            // base_key, replaced_by_session_id, reset_reason, session_state, meta,
            // created_at, updated_at
            Ok(crate::common::fake_databend::rows(&[&[
                "sess_1",
                "agent_1",
                "other_user",
                "title",
                "",
                "",
                "",
                "",
                "{}",
                "{}",
                "2025-01-01T00:00:00Z",
                "2025-01-01T00:00:00Z",
            ]]))
        } else {
            Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: Vec::new(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        }
    });
    let router = api_router(state, "info", &auth_config());

    let response = router
        .oneshot(replay_request("agent_1", "sess_1", "user_1"))
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}
