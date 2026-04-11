//! Tests for GetVariableTool.

use std::sync::Arc;

use bendengine::tools::GetVariableResponse;
use bendengine::tools::GetVariableTool;
use bendengine::types::*;
use tokio_util::sync::CancellationToken;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn ctx() -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: "get_variable".into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
    }
}

fn extract_text(result: &ToolResult) -> std::result::Result<&str, &'static str> {
    match result.content.first() {
        Some(Content::Text { text }) => Ok(text),
        _ => Err("expected text content"),
    }
}

fn mock_found(value: &str) -> GetVariableTool {
    let value = value.to_string();
    let get_fn = Arc::new(move |_name: String| {
        let value = value.clone();
        Box::pin(async move { Ok(GetVariableResponse::Found(value)) })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = std::result::Result<GetVariableResponse, String>,
                        > + Send,
                >,
            >
    });
    GetVariableTool::new(get_fn)
}

fn mock_not_found() -> GetVariableTool {
    let get_fn = Arc::new(move |_name: String| {
        Box::pin(async move { Ok(GetVariableResponse::NotFound) })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = std::result::Result<GetVariableResponse, String>,
                        > + Send,
                >,
            >
    });
    GetVariableTool::new(get_fn)
}

fn mock_error(msg: &str) -> GetVariableTool {
    let msg = msg.to_string();
    let get_fn = Arc::new(move |_name: String| {
        let msg = msg.clone();
        Box::pin(async move { Err(msg) })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = std::result::Result<GetVariableResponse, String>,
                        > + Send,
                >,
            >
    });
    GetVariableTool::new(get_fn)
}

#[tokio::test]
async fn found_returns_value() -> Result {
    let tool = mock_found("secret-token-123");
    let result = tool
        .execute(serde_json::json!({"name": "API_KEY"}), ctx())
        .await?;
    assert_eq!(extract_text(&result)?, "secret-token-123");
    Ok(())
}

#[tokio::test]
async fn not_found_returns_message() -> Result {
    let tool = mock_not_found();
    let result = tool
        .execute(serde_json::json!({"name": "MISSING_KEY"}), ctx())
        .await?;
    let text = extract_text(&result)?;
    assert!(text.contains("MISSING_KEY"));
    assert!(text.contains("not set"));
    Ok(())
}

#[tokio::test]
async fn missing_name_param_returns_error() {
    let tool = mock_found("unused");
    let result = tool.execute(serde_json::json!({}), ctx()).await;
    assert!(result.is_err());
    if let Err(err) = result {
        assert!(err.to_string().contains("name"));
    }
}

#[tokio::test]
async fn empty_name_returns_error() {
    let tool = mock_found("unused");
    let result = tool.execute(serde_json::json!({"name": "  "}), ctx()).await;
    assert!(result.is_err());
    if let Err(err) = result {
        assert!(err.to_string().contains("empty"));
    }
}

#[tokio::test]
async fn callback_error_propagates() {
    let tool = mock_error("storage unavailable");
    let result = tool
        .execute(serde_json::json!({"name": "KEY"}), ctx())
        .await;
    assert!(result.is_err());
    if let Err(err) = result {
        assert!(err.to_string().contains("storage unavailable"));
    }
}

#[tokio::test]
async fn tool_metadata() -> Result {
    let tool = mock_found("x");
    assert_eq!(tool.name(), "get_variable");
    assert_eq!(tool.label(), "Get Variable");
    assert!(!tool.description().is_empty());

    let schema = tool.parameters_schema();
    assert_eq!(schema["required"][0], "name");
    Ok(())
}
