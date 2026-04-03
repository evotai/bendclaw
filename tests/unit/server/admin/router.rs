use axum::body::to_bytes;
use axum::http::Request;
use bendclaw::runtime::SuspendStatus;
use bendclaw::server::admin_router;
use bendclaw::server::AdminState;
use tower::ServiceExt;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn build_runtime(_name: &str) -> std::sync::Arc<bendclaw::runtime::Runtime> {
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
    test_runtime(fake)
}

#[tokio::test]
async fn can_suspend_reports_idle_runtime() {
    let runtime = build_runtime("admin-idle");
    let router = admin_router(AdminState {
        runtime: runtime.clone(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
    });

    let response = router
        .oneshot(
            Request::builder()
                .uri("/admin/v1/can_suspend")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("admin response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: SuspendStatus = serde_json::from_slice(&body).expect("admin payload");
    assert_eq!(payload, SuspendStatus {
        can_suspend: true,
        active_sessions: 0,
        active_tasks: 0,
        active_leases: 0,
    });
}

#[tokio::test]
async fn can_suspend_reports_active_tasks() {
    let runtime = build_runtime("admin-active-task");
    let _task = runtime.track_task();
    let router = admin_router(AdminState {
        runtime: runtime.clone(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
    });

    let response = router
        .oneshot(
            Request::builder()
                .uri("/admin/v1/can_suspend")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("admin response");

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: SuspendStatus = serde_json::from_slice(&body).expect("admin payload");
    assert_eq!(payload, SuspendStatus {
        can_suspend: false,
        active_sessions: 0,
        active_tasks: 1,
        active_leases: 0,
    });
}
