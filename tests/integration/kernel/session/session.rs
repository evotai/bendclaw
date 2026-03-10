use std::sync::Arc;

use anyhow::Result;
use bendclaw_test_harness::mocks::context::test_session;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::llm::MockTurn;

#[tokio::test]
async fn text_response_returns_end_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("Hello!"));
    let session = test_session(llm).await?;
    let result = session.chat("hi", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}

#[tokio::test]
async fn stops_at_max_iterations() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let _result = session.chat("hello", "").await?.finish().await?;
    Ok(())
}

#[tokio::test]
async fn multi_turn_tool_then_text() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("Done!".into()),
    ]));
    let session = test_session(llm).await?;
    let result = session.chat("run echo", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}

#[tokio::test]
async fn multiple_tool_calls_in_single_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCalls(vec![
            ("shell".into(), r#"{"command":"echo a"}"#.into()),
            ("shell".into(), r#"{"command":"echo b"}"#.into()),
        ]),
        MockTurn::Text("Both done.".into()),
    ]));
    let session = test_session(llm).await?;
    let result = session.chat("run two commands", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}
