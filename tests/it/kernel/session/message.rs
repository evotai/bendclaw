use anyhow::bail;
use anyhow::Result;
use bendclaw::execution::fmt::to_chat_messages;
use bendclaw::kernel::Content;
use bendclaw::kernel::ErrorSource;
use bendclaw::kernel::OpType;
use bendclaw::kernel::OperationMeta;
use bendclaw::kernel::Role;
use bendclaw::kernel::ToolCall;
use bendclaw::llm::message::CacheControl;
use bendclaw::sessions::message::MessageMetrics;
use bendclaw::sessions::Message;

// ── Message constructors ──

#[test]
fn message_system() {
    let msg = Message::system("you are helpful");
    assert_eq!(msg.text(), "you are helpful");
    assert_eq!(msg.role(), Some(Role::System));
}

#[test]
fn message_user() {
    let msg = Message::user("hello");
    assert_eq!(msg.text(), "hello");
    assert_eq!(msg.role(), Some(Role::User));
}

#[test]
fn message_user_multimodal() {
    let msg = Message::user_multimodal(vec![
        Content::text("describe this"),
        Content::image("base64data", "image/png"),
    ]);
    assert_eq!(msg.text(), "describe this");
    assert_eq!(msg.role(), Some(Role::User));
}

#[test]
fn message_assistant() {
    let msg = Message::assistant("hi there");
    assert_eq!(msg.text(), "hi there");
    assert_eq!(msg.role(), Some(Role::Assistant));
}

#[test]
fn message_assistant_with_tools() {
    let tc = ToolCall {
        id: "tc_001".into(),
        name: "shell".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    };
    let msg = Message::assistant_with_tools("running command", vec![tc]);
    assert_eq!(msg.text(), "running command");
    assert_eq!(msg.role(), Some(Role::Assistant));
}

#[test]
fn message_tool_result() {
    let msg = Message::tool_result("tc_001", "shell", "file.txt\n", true);
    assert_eq!(msg.text(), "file.txt\n");
    assert_eq!(msg.role(), Some(Role::Tool));
}

#[test]
fn message_tool_result_with_operation() {
    let meta = OperationMeta::new(OpType::Execute);
    let m = Message::tool_result_with_operation("tc1", "shell", "ok", true, meta);
    assert_eq!(m.role(), Some(Role::Tool));
}

#[test]
fn message_compaction() {
    let msg = Message::compaction("summary of conversation");
    assert_eq!(msg.text(), "summary of conversation");
    assert_eq!(msg.role(), Some(Role::System));
}

#[test]
fn message_compaction_with_operation() {
    let meta = OperationMeta::new(OpType::Compaction);
    let m = Message::compaction_with_operation("summary", meta);
    assert_eq!(m.text(), "summary");
}

#[test]
fn message_note() {
    let msg = Message::note("internal note");
    assert_eq!(msg.text(), "internal note");
    assert_eq!(msg.role(), None);
}

#[test]
fn message_memory_has_no_role() {
    let m = Message::Memory {
        operation: "store".into(),
        key: "k".into(),
        value: "v".into(),
    };
    assert_eq!(m.text(), "k: v");
    assert!(m.role().is_none());
}

#[test]
fn message_operation_event() {
    let msg = Message::operation_event(
        "tool",
        "shell",
        "started",
        serde_json::json!({"tool_call_id": "tc_001"}),
    );
    assert_eq!(msg.role(), None);
    assert!(msg.text().contains("[tool:shell] started"));
}

#[test]
fn message_error() {
    let m = Message::error(ErrorSource::Llm, "rate limited");
    assert_eq!(m.text(), "[llm] rate limited");
    assert_eq!(m.role(), Some(Role::Assistant));
}

// ── to_chat_messages ──

#[test]
fn to_chat_messages_filters_memory_and_note() {
    let messages = vec![
        Message::system("sys"),
        Message::user("hi"),
        Message::Memory {
            operation: "store".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::operation_event(
            "llm",
            "reasoning.turn",
            "completed",
            serde_json::json!({"iteration": 1}),
        ),
        Message::note("debug info"),
        Message::assistant("hello"),
    ];
    let chat = to_chat_messages(&messages);
    assert_eq!(chat.len(), 3);
    assert_eq!(chat[0].role, Role::System);
    assert_eq!(chat[1].role, Role::User);
    assert_eq!(chat[2].role, Role::Assistant);
}

#[test]
fn to_chat_messages_compaction_becomes_system() {
    let messages = vec![Message::compaction("summary")];
    let chat = to_chat_messages(&messages);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::System);
    assert!(chat[0].text().contains("summary"));
}

#[test]
fn to_chat_messages_tool_result() {
    let messages = vec![Message::tool_result("tc_001", "shell", "output", true)];
    let chat = to_chat_messages(&messages);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Tool);
    assert_eq!(chat[0].tool_call_id, Some("tc_001".into()));
}

#[test]
fn to_chat_messages_empty() {
    let chat = to_chat_messages(&[]);
    assert!(chat.is_empty());
}

#[test]
fn to_chat_messages_marks_system_and_compaction_for_cache() {
    let messages = vec![Message::system("sys"), Message::compaction("summary")];
    let chat = to_chat_messages(&messages);

    assert_eq!(chat.len(), 2);
    assert_eq!(chat[0].role, Role::System);
    assert_eq!(chat[0].cache_control, Some(CacheControl::Ephemeral));
    assert_eq!(chat[1].role, Role::System);
    assert_eq!(chat[1].cache_control, Some(CacheControl::Ephemeral));
    assert!(chat[1].text().contains("[Previous conversation summary]"));
}

#[test]
fn to_chat_messages_preserves_assistant_tool_calls() {
    let tool_call = bendclaw::llm::message::ToolCall {
        id: "tc_1".into(),
        name: "shell".into(),
        arguments: r#"{"command":"echo hi"}"#.into(),
    };
    let messages = vec![Message::assistant_with_tools("running", vec![tool_call])];

    let chat = to_chat_messages(&messages);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Assistant);
    assert_eq!(chat[0].text(), "running");
    assert_eq!(chat[0].tool_calls.len(), 1);
    assert_eq!(chat[0].tool_calls[0].name, "shell");
}

#[test]
fn to_chat_messages_converts_error_to_assistant_context() {
    let messages = vec![Message::error(
        ErrorSource::Tool("shell".to_string()),
        "permission denied",
    )];

    let chat = to_chat_messages(&messages);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Assistant);
    assert_eq!(chat[0].text(), "[tool:shell] permission denied");
}

// ── Message serde ──

#[test]
fn message_serde_roundtrip_system() -> Result<()> {
    let msg = Message::system("test");
    let json = serde_json::to_string(&msg)?;
    let parsed: Message = serde_json::from_str(&json)?;
    assert_eq!(parsed.text(), "test");
    Ok(())
}

#[test]
fn message_serde_roundtrip_tool_result() -> Result<()> {
    let msg = Message::tool_result("tc_001", "shell", "output", true);
    let json = serde_json::to_string(&msg)?;
    let parsed: Message = serde_json::from_str(&json)?;
    assert_eq!(parsed.text(), "output");
    Ok(())
}

#[test]
fn message_serde_roundtrip_error() -> Result<()> {
    let m = Message::error(ErrorSource::Tool("db".into()), "timeout");
    let json = serde_json::to_string(&m)?;
    let back: Message = serde_json::from_str(&json)?;
    assert_eq!(back.text(), "[tool:db] timeout");
    Ok(())
}

#[test]
fn message_operation_event_serde_roundtrip() -> Result<()> {
    let detail = serde_json::json!({
        "tool_call_id": "tc_001",
        "duration_ms": 12,
        "error": null,
    });
    let msg = Message::operation_event("tool", "shell", "completed", detail.clone());

    assert_eq!(msg.text(), format!("[tool:shell] completed - {}", detail));

    let json = serde_json::to_string(&msg)?;
    let decoded: Message = serde_json::from_str(&json)?;

    match decoded {
        Message::OperationEvent {
            kind,
            name,
            status,
            detail,
        } => {
            assert_eq!(kind, "tool");
            assert_eq!(name, "shell");
            assert_eq!(status, "completed");
            assert_eq!(detail["tool_call_id"], "tc_001");
            assert_eq!(detail["duration_ms"], 12);
            assert!(detail["error"].is_null());
        }
        _ => bail!("expected operation event"),
    }
    Ok(())
}

// ── MessageMetrics ──

#[test]
fn message_metrics_default() {
    let m = MessageMetrics::default();
    assert_eq!(m.input_tokens, 0);
    assert_eq!(m.output_tokens, 0);
    assert_eq!(m.reasoning_tokens, 0);
    assert_eq!(m.ttft_ms, 0);
    assert_eq!(m.duration_ms, 0);
}

#[test]
fn message_metrics_serde_roundtrip() -> Result<()> {
    let m = MessageMetrics {
        input_tokens: 100,
        output_tokens: 50,
        reasoning_tokens: 20,
        ttft_ms: 150,
        duration_ms: 3000,
    };
    let json = serde_json::to_string(&m)?;
    let back: MessageMetrics = serde_json::from_str(&json)?;
    assert_eq!(back.input_tokens, 100);
    assert_eq!(back.output_tokens, 50);
    assert_eq!(back.reasoning_tokens, 20);
    assert_eq!(back.ttft_ms, 150);
    assert_eq!(back.duration_ms, 3000);
    Ok(())
}

#[test]
fn message_metrics_skip_zero_fields() -> Result<()> {
    let m = MessageMetrics::default();
    let json = serde_json::to_string(&m)?;
    assert_eq!(json, "{}");
    Ok(())
}

#[test]
fn message_metrics_partial_fields() -> Result<()> {
    let m = MessageMetrics {
        input_tokens: 10,
        output_tokens: 0,
        reasoning_tokens: 0,
        ttft_ms: 0,
        duration_ms: 500,
    };
    let json = serde_json::to_string(&m)?;
    assert!(json.contains("\"input_tokens\":10"));
    assert!(json.contains("\"duration_ms\":500"));
    assert!(!json.contains("\"output_tokens\""));
    Ok(())
}

#[test]
fn assistant_with_metrics_constructor() {
    let metrics = MessageMetrics {
        input_tokens: 200,
        output_tokens: 100,
        reasoning_tokens: 0,
        ttft_ms: 80,
        duration_ms: 2000,
    };
    let op = OperationMeta::new(OpType::Reasoning);
    let msg = Message::assistant_with_metrics("response", vec![], op, metrics);
    assert_eq!(msg.role(), Some(Role::Assistant));
    assert_eq!(msg.text(), "response");
}

#[test]
fn assistant_with_operation_constructor() {
    let op = OperationMeta::new(OpType::Compaction);
    let msg = Message::assistant_with_operation("compact", vec![], op);
    assert_eq!(msg.role(), Some(Role::Assistant));
    assert_eq!(msg.text(), "compact");
}

#[test]
fn message_serde_roundtrip_memory() -> Result<()> {
    let m = Message::Memory {
        operation: "store".into(),
        key: "k1".into(),
        value: "v1".into(),
    };
    let json = serde_json::to_string(&m)?;
    let back: Message = serde_json::from_str(&json)?;
    assert_eq!(back.text(), "k1: v1");
    Ok(())
}

#[test]
fn message_serde_roundtrip_note() -> Result<()> {
    let m = Message::note("debug info");
    let json = serde_json::to_string(&m)?;
    let back: Message = serde_json::from_str(&json)?;
    assert_eq!(back.text(), "debug info");
    Ok(())
}

#[test]
fn message_serde_roundtrip_compaction() -> Result<()> {
    let m = Message::compaction("summary text");
    let json = serde_json::to_string(&m)?;
    let back: Message = serde_json::from_str(&json)?;
    assert_eq!(back.text(), "summary text");
    Ok(())
}
