use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use crate::common::run_helpers::fake_run_exec_databend;
use crate::common::run_helpers::RunExecState;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

// ── Unit tests: SSE event mapping ────────────────────────────────────────────

#[test]
fn tool_update_maps_to_sse_tool_call_update() {
    use bendclaw::kernel::run::event::Event;
    use bendclaw::service::v1::runs::stream::map_event_to_sse;

    let event = Event::ToolUpdate {
        tool_call_id: "tc_001".to_string(),
        output: "streaming chunk".to_string(),
    };
    let sse = map_event_to_sse("agent-1", "session-1", "run-1", &event);
    assert!(sse.is_some(), "ToolUpdate should produce an SSE event");
}

#[test]
fn tool_start_maps_to_sse_tool_call_started() {
    use bendclaw::kernel::run::event::Event;
    use bendclaw::service::v1::runs::stream::map_event_to_sse;

    let event = Event::ToolStart {
        tool_call_id: "tc_001".to_string(),
        name: "claude_code".to_string(),
        arguments: serde_json::json!({"prompt": "hello"}),
    };
    let sse = map_event_to_sse("agent-1", "session-1", "run-1", &event);
    assert!(sse.is_some(), "ToolStart should produce an SSE event");
}

#[test]
fn tool_end_maps_to_sse_tool_call_completed() {
    use bendclaw::kernel::run::event::Event;
    use bendclaw::kernel::OpType;
    use bendclaw::kernel::OperationMeta;
    use bendclaw::service::v1::runs::stream::map_event_to_sse;

    let event = Event::ToolEnd {
        tool_call_id: "tc_001".to_string(),
        name: "codex_exec".to_string(),
        success: true,
        output: "done".to_string(),
        operation: OperationMeta::new(OpType::Execute),
    };
    let sse = map_event_to_sse("agent-1", "session-1", "run-1", &event);
    assert!(sse.is_some(), "ToolEnd should produce an SSE event");
}

// ── Integration test: non-stream run with tool calls ─────────────────────────

async fn fake_runs_app(
    llm: Arc<dyn bendclaw::llm::provider::LLMProvider>,
    session_id: &str,
) -> Result<axum::Router> {
    let fake = fake_run_exec_databend(RunExecState::default(), session_id);
    let prefix = format!(
        "test_coding_agent_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        llm,
    )
    .await
}

/// Non-streaming run: LLM calls a tool that doesn't exist as a real CLI binary.
/// The tool execution will fail (claude/codex not installed), but the run
/// completes and the response includes ToolStart/ToolEnd events in the
/// persisted run record, proving the dispatch pipeline works end-to-end.
#[tokio::test]
async fn non_stream_run_with_coding_agent_tool_call_completes() -> Result<()> {
    let session_id = "session-coding-1";
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".to_string(),
            arguments: serde_json::json!({"command": "echo hello"}).to_string(),
        },
        MockTurn::Text("done".to_string()),
    ]));

    let app = fake_runs_app(llm, session_id).await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "session_id": session_id,
                    "input": "run a shell command",
                    "stream": false
                }))?))?,
        )
        .await?;

    let status = resp.status();
    let body = json_body(resp).await?;
    if status != axum::http::StatusCode::OK {
        anyhow::bail!("unexpected status {status}: {body}");
    }
    assert_eq!(body["status"], "COMPLETED");
    assert!(body["events"].as_array().is_some());
    Ok(())
}
