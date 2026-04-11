//! Serde round-trip tests for core types.

use bendengine::*;

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: &T,
) {
    let json = serde_json::to_string(value).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*value, back);
}

// ---------------------------------------------------------------------------
// Message variants
// ---------------------------------------------------------------------------

#[test]
fn test_message_user_roundtrip() {
    let msg = Message::User {
        content: vec![Content::Text {
            text: "Hello".into(),
        }],
        timestamp: 123456,
    };
    roundtrip(&msg);
}

#[test]
fn test_message_assistant_roundtrip() {
    let msg = Message::Assistant {
        content: vec![
            Content::Text {
                text: "Hi there".into(),
            },
            Content::ToolCall {
                id: "tc-1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "foo.rs"}),
            },
        ],
        stop_reason: StopReason::ToolUse,
        model: "claude-sonnet".into(),
        provider: "anthropic".into(),
        usage: Usage {
            input: 100,
            output: 50,
            cache_read: 10,
            cache_write: 5,
            total_tokens: 165,
        },
        timestamp: 789,
        error_message: None,
    };
    roundtrip(&msg);
}

#[test]
fn test_message_tool_result_roundtrip() {
    let msg = Message::ToolResult {
        tool_call_id: "tc-1".into(),
        tool_name: "bash".into(),
        content: vec![Content::Text {
            text: "exit code 0".into(),
        }],
        is_error: false,
        timestamp: 999,
    };
    roundtrip(&msg);
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

#[test]
fn test_agent_message_roundtrip() {
    let am = AgentMessage::Llm(Message::user("test prompt"));
    roundtrip(&am);
}

#[test]
fn test_extension_message_roundtrip() {
    let ext = ExtensionMessage::new("status_update", serde_json::json!({"status": "running"}));
    roundtrip(&ext);

    let am = AgentMessage::Extension(ext);
    roundtrip(&am);
}

// ---------------------------------------------------------------------------
// Content variants
// ---------------------------------------------------------------------------

#[test]
fn test_content_variants_roundtrip() {
    roundtrip(&Content::Text {
        text: "hello".into(),
    });
    roundtrip(&Content::Image {
        data: "base64data".into(),
        mime_type: "image/png".into(),
    });
    roundtrip(&Content::Thinking {
        thinking: "let me think...".into(),
        signature: Some("sig123".into()),
    });
    roundtrip(&Content::ToolCall {
        id: "tc-1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({"command": "ls"}),
    });
}

// ---------------------------------------------------------------------------
// Full conversation
// ---------------------------------------------------------------------------

#[test]
fn test_full_conversation_roundtrip() {
    let conversation: Vec<AgentMessage> = vec![
        AgentMessage::Llm(Message::user("Read the file")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "tc-1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "main.rs"}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "mock".into(),
            provider: "mock".into(),
            usage: Usage::default(),
            timestamp: 100,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "read_file".into(),
            content: vec![Content::Text {
                text: "fn main() {}".into(),
            }],
            is_error: false,
            timestamp: 200,
        }),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: "The file contains a main function.".into(),
            }],
            stop_reason: StopReason::Stop,
            model: "mock".into(),
            provider: "mock".into(),
            usage: Usage::default(),
            timestamp: 300,
            error_message: None,
        }),
        AgentMessage::Extension(ExtensionMessage::new(
            "ui_event",
            serde_json::json!({"action": "scroll"}),
        )),
    ];

    let json = serde_json::to_string(&conversation).expect("serialize");
    let back: Vec<AgentMessage> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(conversation, back);
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

#[test]
fn test_execution_limits_roundtrip() {
    use bendengine::context::ExecutionLimits;
    let limits = ExecutionLimits {
        max_turns: 25,
        max_total_tokens: 500_000,
        max_duration: std::time::Duration::from_secs(300),
    };
    let json = serde_json::to_string(&limits).expect("serialize");
    let back: ExecutionLimits = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(limits.max_turns, back.max_turns);
    assert_eq!(limits.max_total_tokens, back.max_total_tokens);
    assert_eq!(limits.max_duration, back.max_duration);
}

#[test]
fn test_tool_execution_strategy_roundtrip() {
    roundtrip(&ToolExecutionStrategy::Sequential);
    roundtrip(&ToolExecutionStrategy::Parallel);
    roundtrip(&ToolExecutionStrategy::Batched { size: 4 });
}
