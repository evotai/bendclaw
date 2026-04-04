use bendclaw::run::map_sdk_message;
use bendclaw::run::run_started_event;
use bendclaw::run::AssistantBlock;
use bendclaw::run::AssistantPayload;
use bendclaw::run::MessagePayload;
use bendclaw::run::RunEventKind;
use bendclaw::run::ToolResultPayload;

#[test]
fn map_all_sdk_message_variants() {
    let run_id = "run-001";
    let session_id = "sess-001";

    let cases: Vec<(bend_agent::SDKMessage, RunEventKind)> = vec![
        (
            bend_agent::SDKMessage::System {
                message: "started".into(),
            },
            RunEventKind::System,
        ),
        (
            bend_agent::SDKMessage::Assistant {
                message: bend_agent::Message {
                    role: bend_agent::MessageRole::Assistant,
                    content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
                },
                usage: None,
            },
            RunEventKind::AssistantMessage,
        ),
        (
            bend_agent::SDKMessage::ToolResult {
                tool_use_id: "t1".into(),
                tool_name: "Read".into(),
                content: "ok".into(),
                is_error: false,
            },
            RunEventKind::ToolResult,
        ),
        (
            bend_agent::SDKMessage::PartialMessage {
                text: "partial".into(),
            },
            RunEventKind::PartialMessage,
        ),
        (
            bend_agent::SDKMessage::CompactBoundary {
                summary: "compacted".into(),
            },
            RunEventKind::CompactBoundary,
        ),
        (
            bend_agent::SDKMessage::Status {
                message: "ok".into(),
            },
            RunEventKind::Status,
        ),
        (
            bend_agent::SDKMessage::TaskNotification {
                task_id: "task-1".into(),
                status: "done".into(),
                message: None,
            },
            RunEventKind::TaskNotification,
        ),
        (
            bend_agent::SDKMessage::RateLimit {
                retry_after_ms: 1000,
                message: "slow down".into(),
            },
            RunEventKind::RateLimit,
        ),
        (
            bend_agent::SDKMessage::Progress {
                message: "50%".into(),
            },
            RunEventKind::Progress,
        ),
        (
            bend_agent::SDKMessage::Error {
                message: "fail".into(),
            },
            RunEventKind::Error,
        ),
        (
            bend_agent::SDKMessage::Result {
                text: "done".into(),
                usage: bend_agent::Usage::default(),
                num_turns: 1,
                cost_usd: 0.01,
                duration_ms: 100,
                messages: vec![],
            },
            RunEventKind::RunFinished,
        ),
    ];

    for (sdk_msg, expected_kind) in cases {
        let event = map_sdk_message(&sdk_msg, run_id, session_id, 1);
        assert_eq!(event.run_id, run_id);
        assert_eq!(event.session_id, session_id);
        assert_eq!(
            std::mem::discriminant(&event.kind),
            std::mem::discriminant(&expected_kind),
        );
    }
}

#[test]
fn run_started_event_has_correct_kind() {
    let event = run_started_event("run-001", "sess-001");
    assert!(matches!(event.kind, RunEventKind::RunStarted));
    assert_eq!(event.turn, 0);
}

#[test]
fn assistant_event_payload_is_typed() {
    let msg = bend_agent::SDKMessage::Assistant {
        message: bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![
                bend_agent::ContentBlock::Text { text: "hi".into() },
                bend_agent::ContentBlock::ToolUse {
                    id: "tool-1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({ "path": "a.txt" }),
                },
            ],
        },
        usage: None,
    };

    let event = map_sdk_message(&msg, "run-001", "sess-001", 1);
    let payload = event.payload_as::<AssistantPayload>().unwrap();
    assert_eq!(payload.role, "assistant");
    assert_eq!(payload.content.len(), 2);
    assert!(matches!(payload.content[0], AssistantBlock::Text { .. }));
    assert!(matches!(payload.content[1], AssistantBlock::ToolUse { .. }));
}

#[test]
fn message_event_payload_is_typed() {
    let msg = bend_agent::SDKMessage::Progress {
        message: "working".into(),
    };
    let event = map_sdk_message(&msg, "run-001", "sess-001", 1);
    let payload = event.payload_as::<MessagePayload>().unwrap();
    assert_eq!(payload.message, "working");
}

#[test]
fn tool_result_event_payload_is_typed() {
    let msg = bend_agent::SDKMessage::ToolResult {
        tool_use_id: "tool-1".into(),
        tool_name: "Read".into(),
        content: "done".into(),
        is_error: false,
    };
    let event = map_sdk_message(&msg, "run-001", "sess-001", 1);
    let payload = event.payload_as::<ToolResultPayload>().unwrap();
    assert_eq!(payload.tool_name, "Read");
    assert_eq!(payload.content, "done");
    assert!(!payload.is_error);
}
