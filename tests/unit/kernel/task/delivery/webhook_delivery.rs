use axum::routing::post;
use axum::Json;
use axum::Router;
use bendclaw::kernel::task::delivery::webhook_delivery::deliver_webhook;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;

fn sample_task() -> TaskRecord {
    TaskRecord {
        id: "task-1".to_string(),
        node_id: "inst-1".to_string(),
        name: "nightly-report".to_string(),
        prompt: "run report".to_string(),
        enabled: true,
        status: "idle".to_string(),
        schedule: TaskSchedule::Every { seconds: 60 },
        delivery: TaskDelivery::None,
        user_id: String::new(),
        scope: "private".to_string(),
        created_by: String::new(),
        last_error: None,
        delete_after_run: false,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at: None,
        lease_token: None,
        lease_node_id: None,
        lease_expires_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

async fn start_webhook_server(status: axum::http::StatusCode) -> String {
    async fn ok(
        Json(payload): Json<serde_json::Value>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        (axum::http::StatusCode::OK, Json(payload))
    }

    async fn fail(
        Json(payload): Json<serde_json::Value>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        (axum::http::StatusCode::BAD_GATEWAY, Json(payload))
    }

    let app = match status {
        axum::http::StatusCode::OK => Router::new().route("/", post(ok)),
        _ => Router::new().route("/", post(fail)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind webhook server");
    let addr = listener.local_addr().expect("webhook addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve webhook server");
    });
    format!("http://{addr}/")
}

#[tokio::test]
async fn reports_success() {
    let client = reqwest::Client::new();
    let url = start_webhook_server(axum::http::StatusCode::OK).await;
    let task = sample_task();

    let (status, error) = deliver_webhook(&client, &url, &task, "ok", Some("done"), None).await;

    assert_eq!(status.as_deref(), Some("ok"));
    assert!(error.is_none());
}

#[tokio::test]
async fn reports_http_failure() {
    let client = reqwest::Client::new();
    let url = start_webhook_server(axum::http::StatusCode::BAD_GATEWAY).await;
    let task = sample_task();

    let (status, error) = deliver_webhook(&client, &url, &task, "error", None, Some("boom")).await;

    assert_eq!(status.as_deref(), Some("failed"));
    assert!(error.as_deref().is_some_and(|value| value.contains("502")));
}
