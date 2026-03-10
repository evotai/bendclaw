use anyhow::Result;
use bendclaw::llm::message::CacheControl;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::message::Content;
use bendclaw::llm::message::Role;
use bendclaw::llm::message::ToolCall;

// ── Role ──

#[test]
fn role_display() {
    assert_eq!(format!("{}", Role::System), "system");
    assert_eq!(format!("{}", Role::User), "user");
    assert_eq!(format!("{}", Role::Assistant), "assistant");
    assert_eq!(format!("{}", Role::Tool), "tool");
}

#[test]
fn role_serde_roundtrip() -> Result<()> {
    let role = Role::Assistant;
    let json = serde_json::to_string(&role)?;
    assert_eq!(json, r#""assistant""#);
    let parsed: Role = serde_json::from_str(&json)?;
    assert_eq!(parsed, role);
    Ok(())
}

// ── Content ──

#[test]
fn content_text() {
    let c = Content::text("hello");
    assert_eq!(c.as_text(), "hello");
}

#[test]
fn content_image_as_text_is_empty() {
    let c = Content::image("base64data", "image/png");
    assert_eq!(c.as_text(), "");
}

#[test]
fn content_serde_roundtrip_text() -> anyhow::Result<()> {
    let c = Content::text("hello");
    let json = serde_json::to_string(&c)?;
    let parsed: Content = serde_json::from_str(&json)?;
    assert_eq!(parsed.as_text(), "hello");
    Ok(())
}

#[test]
fn content_serde_roundtrip_image() -> anyhow::Result<()> {
    let c = Content::image("data123", "image/jpeg");
    let json = serde_json::to_string(&c)?;
    let parsed: Content = serde_json::from_str(&json)?;
    match parsed {
        Content::Image { data, mime_type } => {
            assert_eq!(data, "data123");
            assert_eq!(mime_type, "image/jpeg");
        }
        _ => anyhow::bail!("expected Image"),
    }
    Ok(())
}

// ── ChatMessage constructors ──

#[test]
fn chat_message_system() {
    let msg = ChatMessage::system("you are helpful");
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.text(), "you are helpful");
    assert!(msg.tool_calls.is_empty());
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn chat_message_user() {
    let msg = ChatMessage::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.text(), "hello");
}

#[test]
fn chat_message_user_multimodal() {
    let msg = ChatMessage::user_multimodal(vec![
        Content::text("describe"),
        Content::image("data", "image/png"),
    ]);
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.text(), "describe");
    assert_eq!(msg.content.len(), 2);
}

#[test]
fn chat_message_assistant() {
    let msg = ChatMessage::assistant("hi there");
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.text(), "hi there");
}

#[test]
fn chat_message_assistant_with_tool_calls() {
    let tc = ToolCall {
        id: "tc_001".into(),
        name: "shell".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    };
    let msg = ChatMessage::assistant_with_tool_calls("running", vec![tc]);
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.text(), "running");
    assert_eq!(msg.tool_calls.len(), 1);
    assert_eq!(msg.tool_calls[0].name, "shell");
}

#[test]
fn chat_message_tool_result() {
    let msg = ChatMessage::tool_result("tc_001", "output text");
    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.tool_call_id, Some("tc_001".into()));
    assert_eq!(msg.text(), "output text");
}

// ── Cache control ──

#[test]
fn chat_message_with_cache_control() {
    let msg = ChatMessage::system("prompt").with_cache_control();
    assert_eq!(msg.cache_control, Some(CacheControl::Ephemeral));
}

#[test]
fn chat_message_default_no_cache_control() {
    let msg = ChatMessage::user("hi");
    assert!(msg.cache_control.is_none());
}

// ── text() concatenation ──

#[test]
fn chat_message_text_concatenates_text_parts() {
    let msg = ChatMessage {
        role: Role::User,
        content: vec![Content::text("hello "), Content::text("world")],
        ..Default::default()
    };
    assert_eq!(msg.text(), "hello world");
}

#[test]
fn chat_message_text_skips_images() {
    let msg = ChatMessage {
        role: Role::User,
        content: vec![
            Content::text("see: "),
            Content::image("data", "image/png"),
            Content::text("above"),
        ],
        ..Default::default()
    };
    assert_eq!(msg.text(), "see: above");
}

// ── ToolCall serde ──

#[test]
fn tool_call_serde_roundtrip() -> anyhow::Result<()> {
    let tc = ToolCall {
        id: "tc_001".into(),
        name: "file_read".into(),
        arguments: r#"{"path":"a.rs"}"#.into(),
    };
    let json = serde_json::to_string(&tc)?;
    let parsed: ToolCall = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "tc_001");
    assert_eq!(parsed.name, "file_read");
    assert_eq!(parsed.arguments, r#"{"path":"a.rs"}"#);
    Ok(())
}
