use std::sync::Arc;

use anyhow::Result;
use axum::body::to_bytes;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::TestContext;
use crate::common::setup::chat;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

#[tokio::test]
async fn list_runs_empty() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("run-empty");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["data"].as_array().unwrap().is_empty());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn run_created_after_chat() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("run reply"))).await?;
    let agent_id = uid("run-chat");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "hello").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    let runs = body["data"].as_array().unwrap();
    assert!(!runs.is_empty());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn get_run_by_id() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("run-get");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "test").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    let runs = body["data"].as_array().unwrap();
    let run_id = runs[0]["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/runs/{run_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let run = json_body(resp2).await?;
    assert_eq!(run["id"], run_id.as_str());
    assert_eq!(run["session_id"], session_id.as_str());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn cancel_non_running_run_returns_ok() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("run-cancel");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "test").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    let runs = body["data"].as_array().unwrap();
    let run_id = runs[0]["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs/{run_id}/cancel"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = json_body(resp2).await?;
    assert_eq!(body, serde_json::json!({}));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn run_with_tool_call_has_iterations() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("done".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("run-iter");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "run echo").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    let runs = body["data"].as_array().unwrap();
    assert!(!runs.is_empty());
    let iterations: u64 = runs[0]["iterations"].as_u64().unwrap_or(0);
    assert!(iterations >= 2);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn create_run_non_stream_returns_run_response() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok from run"))).await?;
    let agent_id = uid("run-create-json");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let body = serde_json::json!({
        "session_id": session_id,
        "input": "hello from create_run",
        "stream": false
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await?;
    assert_eq!(json["session_id"], session_id.as_str());
    assert_eq!(json["input"], "hello from create_run");
    assert_eq!(json["output"], "ok from run");
    assert_eq!(json["status"], "COMPLETED");
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn create_run_stream_returns_agno_style_sse() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("streamed reply"))).await?;
    let agent_id = uid("run-create-sse");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let body = serde_json::json!({
        "session_id": session_id,
        "input": "stream me",
        "stream": true
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("event: RunStarted"));
    assert!(text.contains("event: RunContent"));
    assert!(text.contains("event: RunCompleted"));
    assert!(text.contains("\"event\":\"RunStarted\""));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn list_runs_via_agno_style_endpoint() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("run-list-new");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "hello").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/agents/{agent_id}/runs?session_id={session_id}"
                ))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn continue_non_paused_run_returns_conflict() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("run-continue");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let body = serde_json::json!({
        "session_id": session_id,
        "input": "first run",
        "stream": false
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    let run_id = created["id"].as_str().unwrap().to_string();

    let continue_body = serde_json::json!({
        "input": "continue now",
        "stream": false
    });
    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs/{run_id}/continue"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&continue_body)?))?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
    ctx.teardown().await;
    Ok(())
}

// ── RunResponse serde ──

#[test]
fn run_response_serializes_fields() {
    let r = bendclaw::service::v1::runs::RunResponse {
        id: "run-1".into(),
        session_id: "sess-1".into(),
        status: "COMPLETED".into(),
        input: "hello".into(),
        output: "world".into(),
        error: String::new(),
        metrics: serde_json::json!({ "tokens": 10 }),
        stop_reason: "end_turn".into(),
        iterations: 3,
        parent_run_id: String::new(),
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-01T00:01:00Z".into(),
        events: None,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["id"], "run-1");
    assert_eq!(v["session_id"], "sess-1");
    assert_eq!(v["status"], "COMPLETED");
    assert_eq!(v["input"], "hello");
    assert_eq!(v["output"], "world");
    assert_eq!(v["metrics"]["tokens"], 10);
    assert_eq!(v["stop_reason"], "end_turn");
    assert_eq!(v["iterations"], 3);
    assert_eq!(v["created_at"], "2024-01-01T00:00:00Z");
    assert_eq!(v["updated_at"], "2024-01-01T00:01:00Z");
}

#[test]
fn run_response_null_metrics() {
    let r = bendclaw::service::v1::runs::RunResponse {
        id: "run-2".into(),
        session_id: "sess-2".into(),
        status: "RUNNING".into(),
        input: "q".into(),
        output: String::new(),
        error: String::new(),
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        parent_run_id: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
        events: None,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["metrics"].is_null());
    assert_eq!(v["iterations"], 0);
}
