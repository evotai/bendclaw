use bendclaw::kernel::run::fmt::to_chat_messages;
use bendclaw::kernel::Content;
use bendclaw::kernel::ErrorSource;
use bendclaw::kernel::Role;
use bendclaw::sessions::Message;

#[test]
fn to_chat_messages_filters_memory_and_note() {
    let msgs = vec![
        Message::system("sys"),
        Message::Memory {
            operation: "store".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::note("debug"),
        Message::user("hi"),
    ];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 2);
}

#[test]
fn to_chat_messages_compaction_becomes_system() {
    let msgs = vec![Message::compaction("summary")];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::System);
    assert!(chat[0].text().contains("summary"));
}

#[test]
fn to_chat_messages_error_becomes_assistant() {
    let msgs = vec![Message::error(ErrorSource::Llm, "fail")];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Assistant);
    assert!(chat[0].text().contains("fail"));
}

#[test]
fn to_chat_messages_user_multimodal() {
    let msgs = vec![Message::user_multimodal(vec![
        Content::text("hello"),
        Content::image("data", "image/png"),
    ])];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::User);
}

#[test]
fn to_chat_messages_assistant_with_tool_calls() {
    let tc = bendclaw::kernel::ToolCall {
        id: "tc1".into(),
        name: "shell".into(),
        arguments: "{}".into(),
    };
    let msgs = vec![Message::assistant_with_tools("thinking", vec![tc])];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert!(!chat[0].tool_calls.is_empty());
}

#[test]
fn to_chat_messages_tool_result() {
    let msgs = vec![Message::tool_result("tc1", "shell", "output", true)];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Tool);
}

#[test]
fn to_chat_messages_operation_event_filtered() {
    let msgs = vec![Message::operation_event(
        "tool",
        "shell",
        "started",
        serde_json::json!({}),
    )];
    let chat = to_chat_messages(&msgs);
    assert!(chat.is_empty());
}

#[test]
fn to_chat_messages_empty_input() {
    let chat = to_chat_messages(&[]);
    assert!(chat.is_empty());
}

#[test]
fn to_chat_messages_assistant_plain_text_no_tool_calls() {
    let msgs = vec![Message::assistant("plain response")];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].role, Role::Assistant);
    assert!(chat[0].tool_calls.is_empty());
    assert_eq!(chat[0].text(), "plain response");
}

#[test]
fn to_chat_messages_compaction_summary_prefix() {
    let msgs = vec![Message::compaction("important context")];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    assert!(chat[0]
        .text()
        .starts_with("[Previous conversation summary]"));
    assert!(chat[0].text().contains("important context"));
}

#[test]
fn to_chat_messages_preserves_order() {
    let msgs = vec![
        Message::system("sys"),
        Message::user("q1"),
        Message::assistant("a1"),
        Message::user("q2"),
        Message::assistant("a2"),
    ];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 5);
    assert_eq!(chat[0].role, Role::System);
    assert_eq!(chat[1].role, Role::User);
    assert_eq!(chat[2].role, Role::Assistant);
    assert_eq!(chat[3].role, Role::User);
    assert_eq!(chat[4].role, Role::Assistant);
}

#[test]
fn to_chat_messages_only_filtered_types_returns_empty() {
    let msgs = vec![
        Message::Memory {
            operation: "store".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::note("debug note"),
        Message::operation_event("tool", "shell", "started", serde_json::json!({})),
    ];
    let chat = to_chat_messages(&msgs);
    assert!(chat.is_empty());
}

#[test]
fn to_chat_messages_error_format_contains_source() {
    let msgs = vec![Message::error(
        ErrorSource::Tool("shell".into()),
        "tool crashed",
    )];
    let chat = to_chat_messages(&msgs);
    assert_eq!(chat.len(), 1);
    let text = chat[0].text();
    assert!(text.contains("tool crashed"));
    assert!(text.contains("tool"));
}
