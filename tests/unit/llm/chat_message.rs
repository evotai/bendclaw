use anyhow::Result;
use bendclaw::llm::message::CacheControl;
use bendclaw::llm::message::ChatMessage;
use bendclaw::types::Content;
use bendclaw::types::Role;

#[test]
fn chat_message_system() {
    let m = ChatMessage::system("prompt");
    assert_eq!(m.role, Role::System);
    assert_eq!(m.text(), "prompt");
}

#[test]
fn chat_message_user() {
    let m = ChatMessage::user("hello");
    assert_eq!(m.role, Role::User);
    assert_eq!(m.text(), "hello");
}

#[test]
fn chat_message_user_multimodal() {
    let m = ChatMessage::user_multimodal(vec![
        Content::text("hi"),
        Content::image("data", "image/png"),
    ]);
    assert_eq!(m.role, Role::User);
    assert_eq!(m.text(), "hi");
}

#[test]
fn chat_message_assistant() {
    let m = ChatMessage::assistant("response");
    assert_eq!(m.role, Role::Assistant);
    assert_eq!(m.text(), "response");
}

#[test]
fn chat_message_assistant_with_tool_calls() {
    let tc = bendclaw::types::ToolCall {
        id: "tc1".into(),
        name: "shell".into(),
        arguments: "{}".into(),
    };
    let m = ChatMessage::assistant_with_tool_calls("thinking", vec![tc]);
    assert_eq!(m.role, Role::Assistant);
    assert_eq!(m.tool_calls.len(), 1);
}

#[test]
fn chat_message_tool_result() {
    let m = ChatMessage::tool_result("tc1", "output");
    assert_eq!(m.role, Role::Tool);
    assert_eq!(m.tool_call_id.as_deref(), Some("tc1"));
    assert_eq!(m.text(), "output");
}

#[test]
fn chat_message_with_cache_control() {
    let m = ChatMessage::system("prompt").with_cache_control();
    assert_eq!(m.cache_control, Some(CacheControl::Ephemeral));
}

#[test]
fn chat_message_default() {
    let m = ChatMessage::default();
    assert_eq!(m.role, Role::User);
    assert!(m.content.is_empty());
    assert!(m.tool_calls.is_empty());
    assert!(m.tool_call_id.is_none());
    assert!(m.cache_control.is_none());
}

#[test]
fn chat_message_serde_roundtrip() -> Result<()> {
    let m = ChatMessage::system("test").with_cache_control();
    let json = serde_json::to_string(&m)?;
    let back: ChatMessage = serde_json::from_str(&json)?;
    assert_eq!(back.role, Role::System);
    assert_eq!(back.text(), "test");
    assert_eq!(back.cache_control, Some(CacheControl::Ephemeral));
    Ok(())
}

#[test]
fn cache_control_serde_roundtrip() -> Result<()> {
    let cc = CacheControl::Ephemeral;
    let json = serde_json::to_string(&cc)?;
    let back: CacheControl = serde_json::from_str(&json)?;
    assert_eq!(back, CacheControl::Ephemeral);
    Ok(())
}
