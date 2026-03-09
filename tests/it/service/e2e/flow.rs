//! End-to-end integration tests: full agent flow with mock LLM and real Databend.

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use serde_json::Value;
use tower::ServiceExt;

use crate::common::setup::TestContext;
use crate::common::setup::chat;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

async fn get_runs(
    app: &axum::Router,
    agent_id: &str,
    session_id: &str,
    user: &str,
) -> Result<Vec<Value>> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let data: Value = json_body(resp).await?;
    Ok(data["data"]
        .as_array()
        .context("expected runs array")?
        .clone())
}

async fn get_run_detail(
    app: &axum::Router,
    agent_id: &str,
    run_id: &str,
    user: &str,
) -> Result<Value> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/runs/{run_id}"))
                .header("x-user-id", user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    json_body(resp).await
}

async fn get_sessions(app: &axum::Router, agent_id: &str, user: &str) -> Result<Vec<Value>> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions"))
                .header("x-user-id", user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let data: Value = json_body(resp).await?;
    Ok(data["data"]
        .as_array()
        .context("expected sessions array")?
        .clone())
}

async fn update_config(app: &axum::Router, agent_id: &str, user: &str, body: Value) -> Result<()> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("content-type", "application/json")
                .header("x-user-id", user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn e2e_tool_call_persists_full_message_chain() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hello"}"#.into(),
        },
        MockTurn::Text("Command executed successfully.".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-tc");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    let resp = chat(&app, &agent_id, &session_id, &user, "run echo hello").await?;
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["message"], "Command executed successfully.");

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    assert_eq!(runs.len(), 1);
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    assert_eq!(runs[0]["input"], "run echo hello");
    assert_eq!(runs[0]["output"], "Command executed successfully.");

    let detail = get_run_detail(&app, &agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("missing run events")?;
    assert!(events.iter().any(|e| e["event"] == "ToolStart"));
    assert!(events.iter().any(|e| e["event"] == "ToolEnd"));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_multi_turn_accumulates_history() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("reply"));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-mt");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;

    chat(&app, &agent_id, &session_id, &user, "first question").await?;
    assert_eq!(
        get_runs(&app, &agent_id, &session_id, &user).await?.len(),
        1
    );

    chat(&app, &agent_id, &session_id, &user, "second question").await?;
    assert_eq!(
        get_runs(&app, &agent_id, &session_id, &user).await?.len(),
        2
    );

    chat(&app, &agent_id, &session_id, &user, "third question").await?;
    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    assert_eq!(runs.len(), 3);

    let inputs: Vec<&str> = runs.iter().filter_map(|r| r["input"].as_str()).collect();
    assert!(inputs.contains(&"first question"));
    assert!(inputs.contains(&"second question"));
    assert!(inputs.contains(&"third question"));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_audit_trail_records_all_messages() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        },
        MockTurn::Text("Listed files.".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-audit");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "list files").await?;

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = get_run_detail(&app, &agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    assert!(
        events.len() >= 6,
        "expected enough events, got {}",
        events.len()
    );
    assert!(events.iter().any(|e| e["event"] == "ReasonStart"));
    assert!(events.iter().any(|e| e["event"] == "ToolEnd"));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_phase1_audit_events_are_persisted() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("audit ok"));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-phase1");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "show audit trail").await?;

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = get_run_detail(&app, &agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    for name in [
        "run.started",
        "prompt.built",
        "turn.started",
        "llm.request",
        "llm.response",
        "turn.completed",
        "run.completed",
    ] {
        assert!(
            events.iter().any(|e| e["event"] == name),
            "missing audit event: {name}"
        );
    }

    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_session_lifecycle() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-ses");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;

    let before = get_sessions(&app, &agent_id, &user).await?;
    assert!(!before.iter().any(|s| s["id"].as_str() == Some(&session_id)));

    chat(&app, &agent_id, &session_id, &user, "hello world").await?;

    let after = get_sessions(&app, &agent_id, &user).await?;
    let session = after
        .iter()
        .find(|s| s["id"].as_str() == Some(&session_id))
        .context("session not found")?;
    assert_eq!(session["title"], "hello world");

    chat(&app, &agent_id, &session_id, &user, "follow up").await?;
    let final_sessions = get_sessions(&app, &agent_id, &user).await?;
    let count = final_sessions
        .iter()
        .filter(|s| s["id"].as_str() == Some(&session_id))
        .count();
    assert_eq!(count, 1);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_parallel_tool_calls_all_persisted() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCalls(vec![
            ("shell".into(), r#"{"command":"echo a"}"#.into()),
            ("shell".into(), r#"{"command":"echo b"}"#.into()),
        ]),
        MockTurn::Text("Both commands done.".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-par");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    let resp = chat(&app, &agent_id, &session_id, &user, "run two commands").await?;
    assert_eq!(resp["message"], "Both commands done.");

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = get_run_detail(&app, &agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    let tool_start_count = events.iter().filter(|e| e["event"] == "ToolStart").count();
    let tool_end_count = events.iter().filter(|e| e["event"] == "ToolEnd").count();
    assert_eq!(tool_start_count, 2);
    assert_eq!(tool_end_count, 2);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_tool_call_persists_operation_events_with_structured_detail() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hello"}"#.into(),
        },
        MockTurn::Text("Command executed successfully.".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-op");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "run echo hello").await?;

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = get_run_detail(&app, &agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    let has_tool_started = events.iter().any(|e| {
        e["event"] == "ToolStart"
            && e["payload"]["type"] == "ToolStart"
            && e["payload"]["data"]["tool_call_id"].is_string()
            && e["payload"]["data"]["arguments"].is_object()
    });
    assert!(has_tool_started, "missing tool start event");

    let has_tool_completed = events.iter().any(|e| {
        e["event"] == "ToolEnd"
            && e["payload"]["type"] == "ToolEnd"
            && e["payload"]["data"]["tool_call_id"].is_string()
            && e["payload"]["data"]["success"].is_boolean()
    });
    assert!(has_tool_completed, "missing tool end event");

    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_system_prompt_flows_through_config() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("I am a SQL expert."));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-sys");
    let user = uid("user");
    let session_id = uid("session");

    setup_agent(&app, &agent_id, &user).await?;
    update_config(
        &app,
        &agent_id,
        &user,
        serde_json::json!({
            "system_prompt": "You are a SQL expert. Always respond with SQL queries."
        }),
    )
    .await?;

    let resp = chat(&app, &agent_id, &session_id, &user, "create a users table").await?;
    assert_eq!(resp["message"], "I am a SQL expert.");

    let runs = get_runs(&app, &agent_id, &session_id, &user).await?;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["output"], "I am a SQL expert.");
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_session_isolation() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("reply"));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-iso");
    let user = uid("user");
    let session_a = uid("ses-a");
    let session_b = uid("ses-b");

    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_a, &user, "message for A").await?;
    chat(&app, &agent_id, &session_b, &user, "message for B").await?;

    let runs_a = get_runs(&app, &agent_id, &session_a, &user).await?;
    let runs_b = get_runs(&app, &agent_id, &session_b, &user).await?;
    assert_eq!(runs_a.len(), 1);
    assert_eq!(runs_b.len(), 1);

    assert_eq!(runs_a[0]["input"], "message for A");
    assert_eq!(runs_b[0]["input"], "message for B");
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn e2e_user_isolation() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("hello"));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("e2e-uiso");
    let user_a = uid("user-a");
    let user_b = uid("user-b");
    let session_a = uid("ses-a");
    let session_b = uid("ses-b");

    setup_agent(&app, &agent_id, &user_a).await?;
    chat(&app, &agent_id, &session_a, &user_a, "from user A").await?;
    chat(&app, &agent_id, &session_b, &user_b, "from user B").await?;

    let sessions_a = get_sessions(&app, &agent_id, &user_a).await?;
    let sessions_b = get_sessions(&app, &agent_id, &user_b).await?;
    assert!(sessions_a
        .iter()
        .any(|s| s["id"].as_str() == Some(&session_a)));
    assert!(!sessions_a
        .iter()
        .any(|s| s["id"].as_str() == Some(&session_b)));
    assert!(sessions_b
        .iter()
        .any(|s| s["id"].as_str() == Some(&session_b)));
    assert!(!sessions_b
        .iter()
        .any(|s| s["id"].as_str() == Some(&session_a)));

    assert_eq!(
        get_runs(&app, &agent_id, &session_a, &user_a).await?.len(),
        1
    );
    assert_eq!(
        get_runs(&app, &agent_id, &session_b, &user_b).await?.len(),
        1
    );
    ctx.teardown().await;
    Ok(())
}
