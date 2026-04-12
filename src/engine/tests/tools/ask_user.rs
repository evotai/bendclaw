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

fn single_question_params() -> serde_json::Value {
    serde_json::json!({
        "questions": [{
            "question": "Which cache strategy?",
            "header": "Cache",
            "options": [
                { "label": "In-memory (Recommended)", "description": "Zero deps, HashMap + TTL" },
                { "label": "Redis", "description": "Shared across instances" }
            ]
        }]
    })
}

fn two_question_params() -> serde_json::Value {
    serde_json::json!({
        "questions": [
            {
                "question": "Which cache strategy?",
                "header": "Cache",
                "options": [
                    { "label": "In-memory", "description": "Zero deps" },
                    { "label": "Redis", "description": "Shared" }
                ]
            },
            {
                "question": "Which auth method?",
                "header": "Auth",
                "options": [
                    { "label": "OAuth", "description": "Delegated" },
                    { "label": "JWT", "description": "Stateless" }
                ]
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Response formatting
// ---------------------------------------------------------------------------

#[tokio::test]
async fn answered_single_question() {
    let tool = make_tool(AskUserResponse::Answered(vec![AskUserAnswer {
        header: "Cache".into(),
        question: "Which cache strategy?".into(),
        answer: "Redis".into(),
    }]));
    let result = tool
        .execute(single_question_params(), ctx())
        .await
        .unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("User answered your questions:"));
    assert!(text.contains("Which cache strategy? → Redis"));
}

#[tokio::test]
async fn answered_multiple_questions() {
    let tool = make_tool(AskUserResponse::Answered(vec![
        AskUserAnswer {
            header: "Cache".into(),
            question: "Which cache strategy?".into(),
            answer: "In-memory".into(),
        },
        AskUserAnswer {
            header: "Auth".into(),
            question: "Which auth method?".into(),
            answer: "JWT".into(),
        },
    ]));
    let result = tool
        .execute(two_question_params(), ctx())
        .await
        .unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("Which cache strategy? → In-memory"));
    assert!(text.contains("Which auth method? → JWT"));
}

#[tokio::test]
async fn skipped_response() {
    let tool = make_tool(AskUserResponse::Skipped);
    let result = tool
        .execute(single_question_params(), ctx())
        .await
        .unwrap_or_else(|e| panic!("unexpected error: {e}"));
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.contains("Proceed with your best judgment"));
}

#[tokio::test]
async fn callback_error_propagates() {
    let tool = make_error_tool("user disconnected");
    let result = tool.execute(single_question_params(), ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("user disconnected"));
}

// ---------------------------------------------------------------------------
// Validation: questions count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zero_questions_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({ "questions": [] });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("1-4 items"));
}

#[tokio::test]
async fn five_questions_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [
            { "question": "Q1?", "header": "H1", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q2?", "header": "H2", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q3?", "header": "H3", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q4?", "header": "H4", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q5?", "header": "H5", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]}
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("1-4 items"));
}

#[tokio::test]
async fn four_questions_accepted() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [
            { "question": "Q1?", "header": "H1", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q2?", "header": "H2", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q3?", "header": "H3", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Q4?", "header": "H4", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]}
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Validation: field emptiness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_question_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "   ", "header": "H",
            "options": [
                { "label": "A", "description": "a" },
                { "label": "B", "description": "b" }
            ]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("question must not be empty"));
}

#[tokio::test]
async fn empty_header_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "Pick?", "header": "",
            "options": [
                { "label": "A", "description": "a" },
                { "label": "B", "description": "b" }
            ]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("header must not be empty"));
}

#[tokio::test]
async fn empty_label_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "Pick?", "header": "H",
            "options": [
                { "label": "", "description": "a" },
                { "label": "B", "description": "b" }
            ]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("label must not be empty"));
}

#[tokio::test]
async fn empty_description_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "Pick?", "header": "H",
            "options": [
                { "label": "A", "description": "a" },
                { "label": "B", "description": "  " }
            ]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("description must not be empty"));
}

// ---------------------------------------------------------------------------
// Validation: options count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn too_few_options_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "Pick?", "header": "H",
            "options": [{ "label": "Only", "description": "one" }]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("2-4 items"));
}

#[tokio::test]
async fn too_many_options_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [{
            "question": "Pick?", "header": "H",
            "options": [
                { "label": "A", "description": "a" },
                { "label": "B", "description": "b" },
                { "label": "C", "description": "c" },
                { "label": "D", "description": "d" },
                { "label": "E", "description": "e" }
            ]
        }]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("2-4 items"));
}

// ---------------------------------------------------------------------------
// Validation: duplicates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_question_text_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({
        "questions": [
            { "question": "Same?", "header": "H1", "options": [
                { "label": "A", "description": "a" }, { "label": "B", "description": "b" }
            ]},
            { "question": "Same?", "header": "H2", "options": [
                { "label": "C", "description": "c" }, { "label": "D", "description": "d" }
            ]}
        ]
    });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("duplicate"));
}

// ---------------------------------------------------------------------------
// Validation: invalid JSON
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_json_rejected() {
    let tool = make_tool(AskUserResponse::Skipped);
    let params = serde_json::json!({ "wrong_field": true });
    let result = tool.execute(params, ctx()).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_metadata() {
    let tool = make_tool(AskUserResponse::Skipped);
    assert_eq!(tool.name(), "ask_user");
    assert_eq!(tool.label(), "Ask User");
    assert!(!tool.description().is_empty());

    let schema = tool.parameters_schema();
    assert_eq!(schema["required"], serde_json::json!(["questions"]));
}
