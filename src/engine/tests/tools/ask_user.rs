//! Tests for the AskUser tool.

use std::sync::Arc;

use bendengine::tools::*;
use bendengine::types::*;
use tokio_util::sync::CancellationToken;

fn ctx() -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: "ask_user".into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
    }
}

fn make_tool(response: AskUserResponse) -> AskUserTool {
    let response = Arc::new(response);
    let ask_fn: AskUserFn = Arc::new(move |_req| {
        let r = response.clone();
        Box::pin(async move { Ok((*r).clone()) })
    });
    AskUserTool::new(ask_fn)
}

fn make_error_tool(msg: &str) -> AskUserTool {
    let msg = msg.to_string();
    let ask_fn: AskUserFn = Arc::new(move |_req| {
        let m = msg.clone();
        Box::pin(async move { Err(m) })
    });
    AskUserTool::new(ask_fn)
}

fn valid_params() -> serde_json::Value {
    serde_json::json!({
        "question": "Which cache strategy?",
        "options": [
            { "label": "In-memory (Recommended)", "description": "Zero deps, HashMap + TTL" },
            { "label": "Redis", "description": "Shared across instances" }
        ]
    })
}

#[tokio::test]
async fn selected_response() {
    let tool = make_tool(AskUserResponse::Selected("Redis".into()));
    let result = tool.execute(valid_params(), ctx()).await;
    let result = result.unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("User selected: Redis"));
}

#[tokio::test]
async fn custom_response() {
    let tool = make_tool(AskUserResponse::Custom("Use SQLite instead".into()));
    let result = tool.execute(valid_params(), ctx()).await;
    let result = result.unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("User provided custom input: Use SQLite instead"));
}

#[tokio::test]
async fn skipped_response() {
    let tool = make_tool(AskUserResponse::Skipped);
    let result = tool.execute(valid_params(), ctx()).await;
    let result = result.unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("Proceed with your best judgment"));
}

#[tokio::test]
async fn callback_error_propagates() {
    let tool = make_error_tool("user disconnected");
    let result = tool.execute(valid_params(), ctx()).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("user disconnected"));
}

#[tokio::test]
async fn too_few_options_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "question": "Pick one?",
        "options": [
            { "label": "Only one", "description": "Not enough" }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("2-4 items"));
}

#[tokio::test]
async fn too_many_options_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "question": "Pick one?",
        "options": [
            { "label": "A", "description": "a" },
            { "label": "B", "description": "b" },
            { "label": "C", "description": "c" },
            { "label": "D", "description": "d" },
            { "label": "E", "description": "e" }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("2-4 items"));
}

#[tokio::test]
async fn invalid_json_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({ "wrong_field": true });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exactly_four_options_accepted() {
    let tool = make_tool(AskUserResponse::Selected("C".into()));
    let params = serde_json::json!({
        "question": "Pick one?",
        "options": [
            { "label": "A", "description": "a" },
            { "label": "B", "description": "b" },
            { "label": "C", "description": "c" },
            { "label": "D", "description": "d" }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tool_metadata() {
    let tool = make_tool(AskUserResponse::Skipped);
    assert_eq!(tool.name(), "ask_user");
    assert_eq!(tool.label(), "Ask User");
    assert!(!tool.description().is_empty());

    let schema = tool.parameters_schema();
    assert_eq!(
        schema["required"],
        serde_json::json!(["question", "options"])
    );
}

#[tokio::test]
async fn empty_question_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "question": "   ",
        "options": [
            { "label": "A", "description": "a" },
            { "label": "B", "description": "b" }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("question must not be empty"));
}

#[tokio::test]
async fn empty_label_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "question": "Pick one?",
        "options": [
            { "label": "", "description": "a" },
            { "label": "B", "description": "b" }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("option[0].label must not be empty"));
}

#[tokio::test]
async fn empty_description_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "question": "Pick one?",
        "options": [
            { "label": "A", "description": "a" },
            { "label": "B", "description": "  " }
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("option[1].description must not be empty"));
}
