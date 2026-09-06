//! Tests for the generic host-delegated tool (`HostTool`).

use std::sync::Arc;

use async_trait::async_trait;
use evotengine::host::*;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

fn ctx() -> ToolContext {
    ToolContext {
        tool_call_id: "call-1".into(),
        tool_name: "plan".into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
        cwd: std::path::PathBuf::new(),
        path_guard: Arc::new(evotengine::PathGuard::open()),
        spill: None,
        supports_image: true,
    }
}

fn spec() -> HostToolSpec {
    HostToolSpec {
        name: "plan".into(),
        label: "Plan".into(),
        description: "Manage a structured execution plan.".into(),
        parameters_schema: serde_json::json!({ "type": "object" }),
        prompt_snippet: Some("track a multi-step plan".into()),
        name_aliases: vec![("claude".into(), "Plan".into())],
    }
}

/// A host bridge that returns a fixed response, capturing the call it received.
struct StubHost {
    response: Result<HostToolResponse, HostError>,
    captured: std::sync::Mutex<Option<HostToolCall>>,
}

impl StubHost {
    fn ok(resp: HostToolResponse) -> Arc<Self> {
        Arc::new(Self {
            response: Ok(resp),
            captured: std::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl HostBridge for StubHost {
    async fn execute_tool(&self, call: HostToolCall) -> Result<HostToolResponse, HostError> {
        if let Ok(mut slot) = self.captured.lock() {
            *slot = Some(call);
        }
        match &self.response {
            Ok(r) => Ok(r.clone()),
            Err(HostError::Closed) => Err(HostError::Closed),
            Err(HostError::Cancelled) => Err(HostError::Cancelled),
            Err(HostError::Failed(m)) => Err(HostError::Failed(m.clone())),
        }
    }
}

struct WaitingHost {
    started: CancellationToken,
    dropped: CancellationToken,
}

#[async_trait]
impl HostBridge for WaitingHost {
    async fn execute_tool(&self, _call: HostToolCall) -> Result<HostToolResponse, HostError> {
        let _guard = self.dropped.clone().drop_guard();
        self.started.cancel();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancellation_releases_a_host_that_never_replies() -> Result<(), Box<dyn std::error::Error>>
{
    let started = CancellationToken::new();
    let dropped = CancellationToken::new();
    let tool = HostTool::new(
        spec(),
        Arc::new(WaitingHost {
            started: started.clone(),
            dropped: dropped.clone(),
        }),
    );
    let context = ctx();
    let cancel = context.cancel.clone();
    let run = tokio::spawn(async move { tool.execute(serde_json::json!({}), context).await });
    tokio::time::timeout(std::time::Duration::from_secs(1), started.cancelled()).await?;
    cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), run).await??;
    assert!(matches!(result, Err(ToolError::Cancelled)));
    assert!(dropped.is_cancelled());
    Ok(())
}

#[test]
fn spec_maps_onto_agent_tool_metadata() {
    let host = StubHost::ok(HostToolResponse::text("ok"));
    let tool = HostTool::new(spec(), host);

    assert_eq!(tool.name(), "plan");
    assert_eq!(tool.label(), "Plan");
    assert_eq!(tool.prompt_snippet(), Some("track a multi-step plan"));
    // Claude sees the aliased name.
    assert_eq!(tool.resolve_name("claude-sonnet-4"), "Plan");
    // A call using the alias still matches.
    assert!(tool.matches_call_name("Plan"));
    assert!(tool.matches_call_name("plan"));
}

#[tokio::test]
async fn forwards_call_and_returns_result() {
    let host = StubHost::ok(HostToolResponse {
        content: vec![Content::Text {
            text: "plan updated".into(),
        }],
        details: serde_json::json!({ "tasks": [{ "id": 1, "status": "pending" }] }),
        is_error: false,
    });
    let tool = HostTool::new(spec(), host.clone());

    let params = serde_json::json!({ "action": "create" });
    let result = tool.execute(params.clone(), ctx()).await.expect("ok");

    assert_eq!(result.content, vec![Content::Text {
        text: "plan updated".into()
    }]);
    assert_eq!(result.details["tasks"][0]["id"], 1);

    // The bridge received the correlated call.
    let captured = host.captured.lock().expect("lock").clone().expect("call");
    assert_eq!(captured.tool_name, "plan");
    assert_eq!(captured.tool_call_id, "call-1");
    assert_eq!(captured.arguments, params);
}

#[tokio::test]
async fn error_result_becomes_tool_error() {
    let host = StubHost::ok(HostToolResponse {
        content: vec![Content::Text {
            text: "user rejected the plan".into(),
        }],
        details: serde_json::Value::Null,
        is_error: true,
    });
    let tool = HostTool::new(spec(), host);

    let err = tool
        .execute(serde_json::json!({}), ctx())
        .await
        .expect_err("should surface as tool error");
    match err {
        ToolError::Failed(msg) => assert_eq!(msg, "user rejected the plan"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn transport_cancel_maps_to_cancelled() {
    let host = Arc::new(StubHost {
        response: Err(HostError::Cancelled),
        captured: std::sync::Mutex::new(None),
    });
    let tool = HostTool::new(spec(), host);

    let err = tool
        .execute(serde_json::json!({}), ctx())
        .await
        .expect_err("should error");
    assert!(matches!(err, ToolError::Cancelled));
}

#[tokio::test]
async fn transport_closed_maps_to_failed() {
    let host = Arc::new(StubHost {
        response: Err(HostError::Closed),
        captured: std::sync::Mutex::new(None),
    });
    let tool = HostTool::new(spec(), host);

    let err = tool
        .execute(serde_json::json!({}), ctx())
        .await
        .expect_err("should error");
    assert!(matches!(err, ToolError::Failed(_)));
}
