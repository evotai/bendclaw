use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::llm::MockTurn;
use bendclaw_test_harness::setup::chat;
use bendclaw_test_harness::setup::json_body;
use bendclaw_test_harness::setup::setup_agent;
use bendclaw_test_harness::setup::uid;
use bendclaw_test_harness::setup::TestContext;
use tower::ServiceExt;

#[tokio::test]
async fn list_traces_empty() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("tr-empty");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["data"].is_array());
    Ok(())
}

#[tokio::test]
async fn traces_created_after_chat() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("traced reply")))
        .await?;
    let agent_id = uid("tr-chat");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "hello").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    let traces = body["data"].as_array().context("expected data array")?;
    assert!(!traces.is_empty());
    Ok(())
}

#[tokio::test]
async fn traces_summary() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("tr-sum");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "test").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces/summary"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["trace_count"].is_number());
    Ok(())
}

#[tokio::test]
async fn get_trace_by_id() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("done".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("tr-get");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "run echo").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    let traces = body["data"].as_array().context("expected data array")?;
    assert!(!traces.is_empty());
    let trace_id = traces[0]["trace_id"]
        .as_str()
        .context("missing trace_id")?
        .to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces/{trace_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let detail = json_body(resp2).await?;
    assert_eq!(detail["trace"]["trace_id"], trace_id.as_str());
    assert!(detail["spans"].is_array());
    Ok(())
}

#[tokio::test]
async fn traces_filter_by_session() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("tr-flt");
    let user = uid("user");
    let session_a = uid("ses-a");
    let session_b = uid("ses-b");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_a, &user, "msg a").await?;
    chat(&app, &agent_id, &session_b, &user, "msg b").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/agents/{agent_id}/traces?session_id={session_a}"
                ))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    let traces = body["data"].as_array().context("expected data array")?;
    assert!(traces.iter().all(|t| t["session_id"] == session_a.as_str()));
    Ok(())
}

// ── TraceResponse serde ──

#[test]
fn trace_response_serializes_fields() -> anyhow::Result<()> {
    let t = bendclaw::service::v1::traces::TraceResponse {
        trace_id: "tr-1".into(),
        run_id: "run-1".into(),
        session_id: "sess-1".into(),
        name: "my-trace".into(),
        status: "completed".into(),
        duration_ms: 123,
        input_tokens: 10,
        output_tokens: 20,
        total_cost: 0.005,
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&t)?;
    assert_eq!(v["trace_id"], "tr-1");
    assert_eq!(v["run_id"], "run-1");
    assert_eq!(v["session_id"], "sess-1");
    assert_eq!(v["name"], "my-trace");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["duration_ms"], 123);
    assert_eq!(v["input_tokens"], 10);
    assert_eq!(v["output_tokens"], 20);
    assert_eq!(v["created_at"], "2024-01-01T00:00:00Z");
    Ok(())
}

#[test]
fn trace_response_zero_cost() -> anyhow::Result<()> {
    let t = bendclaw::service::v1::traces::TraceResponse {
        trace_id: "tr-2".into(),
        run_id: "run-2".into(),
        session_id: "sess-2".into(),
        name: "cheap".into(),
        status: "running".into(),
        duration_ms: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_cost: 0.0,
        created_at: String::new(),
    };
    let v = serde_json::to_value(&t)?;
    assert_eq!(v["total_cost"], 0.0);
    assert_eq!(v["duration_ms"], 0);
    Ok(())
}

// ── TracesQuery deserialization ──

#[test]
fn traces_query_defaults() {
    let q = bendclaw::service::v1::traces::TracesQuery::default();
    assert!(q.session_id.is_none());
    assert!(q.run_id.is_none());
    assert!(q.user_id.is_none());
    assert!(q.status.is_none());
    assert!(q.start_time.is_none());
    assert!(q.end_time.is_none());
}

#[test]
fn traces_query_all_filters() -> anyhow::Result<()> {
    let q: bendclaw::service::v1::traces::TracesQuery = serde_json::from_str(
        r#"{
            "session_id": "sess-1",
            "run_id": "run-1",
            "user_id": "user-1",
            "status": "completed",
            "start_time": "2024-01-01T00:00:00Z",
            "end_time": "2024-12-31T23:59:59Z"
        }"#,
    )?;
    assert_eq!(q.session_id.as_deref(), Some("sess-1"));
    assert_eq!(q.run_id.as_deref(), Some("run-1"));
    assert_eq!(q.user_id.as_deref(), Some("user-1"));
    assert_eq!(q.status.as_deref(), Some("completed"));
    assert_eq!(q.start_time.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(q.end_time.as_deref(), Some("2024-12-31T23:59:59Z"));
    Ok(())
}

#[test]
fn traces_query_partial_filters() -> anyhow::Result<()> {
    let q: bendclaw::service::v1::traces::TracesQuery =
        serde_json::from_str(r#"{"status": "running"}"#)?;
    assert_eq!(q.status.as_deref(), Some("running"));
    assert!(q.session_id.is_none());
    assert!(q.run_id.is_none());
    Ok(())
}

// ── TraceDetailResponse serde ──

#[test]
fn trace_detail_response_serializes() -> anyhow::Result<()> {
    let detail = bendclaw::service::v1::traces::TraceDetailResponse {
        trace: bendclaw::service::v1::traces::TraceResponse {
            trace_id: "tr-3".into(),
            run_id: "run-3".into(),
            session_id: "sess-3".into(),
            name: "detail-trace".into(),
            status: "completed".into(),
            duration_ms: 500,
            input_tokens: 100,
            output_tokens: 200,
            total_cost: 0.01,
            created_at: "2024-06-01T00:00:00Z".into(),
        },
        spans: vec![],
    };
    let v = serde_json::to_value(&detail)?;
    assert_eq!(v["trace"]["trace_id"], "tr-3");
    assert_eq!(v["trace"]["status"], "completed");
    assert!(v["spans"]
        .as_array()
        .context("expected spans array")?
        .is_empty());
    Ok(())
}
