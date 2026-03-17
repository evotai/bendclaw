//! Tests for AgentEvent, CliAgent::parse_events, and SSE mapping.

use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::tools::cli_agent::AgentEvent;
use bendclaw::service::v1::runs::stream::map_agent_event;
use bendclaw::service::v1::runs::stream::map_event_to_sse;

// ── AgentEvent serde roundtrip ───────────────────────────────────────────────

fn roundtrip(event: AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(&event).unwrap();
    serde_json::from_str(&json).unwrap()
}

#[test]
fn agent_event_text_roundtrip() {
    let e = AgentEvent::Text {
        content: "hello".into(),
    };
    assert!(matches!(roundtrip(e), AgentEvent::Text { content } if content == "hello"));
}

#[test]
fn agent_event_thinking_roundtrip() {
    let e = AgentEvent::Thinking {
        content: "hmm".into(),
    };
    assert!(matches!(roundtrip(e), AgentEvent::Thinking { content } if content == "hmm"));
}

#[test]
fn agent_event_tool_use_roundtrip() {
    let e = AgentEvent::ToolUse {
        tool_name: "Read".into(),
        tool_use_id: "tu_1".into(),
        input: serde_json::json!({"path": "/tmp/x"}),
    };
    let back = roundtrip(e);
    match back {
        AgentEvent::ToolUse {
            tool_name,
            tool_use_id,
            input,
        } => {
            assert_eq!(tool_name, "Read");
            assert_eq!(tool_use_id, "tu_1");
            assert_eq!(input["path"], "/tmp/x");
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn agent_event_tool_result_roundtrip() {
    let e = AgentEvent::ToolResult {
        tool_use_id: "tu_1".into(),
        success: true,
        output: "ok".into(),
    };
    let back = roundtrip(e);
    match back {
        AgentEvent::ToolResult {
            tool_use_id,
            success,
            output,
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert!(success);
            assert_eq!(output, "ok");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn agent_event_system_roundtrip() {
    let e = AgentEvent::System {
        subtype: "init".into(),
        metadata: serde_json::json!({"model": "claude"}),
    };
    let back = roundtrip(e);
    match back {
        AgentEvent::System { subtype, metadata } => {
            assert_eq!(subtype, "init");
            assert_eq!(metadata["model"], "claude");
        }
        _ => panic!("expected System"),
    }
}

#[test]
fn agent_event_error_roundtrip() {
    let e = AgentEvent::Error {
        message: "boom".into(),
    };
    assert!(matches!(roundtrip(e), AgentEvent::Error { message } if message == "boom"));
}

#[test]
fn agent_event_kind_returns_correct_tag() {
    assert_eq!(AgentEvent::Text { content: "".into() }.kind(), "text");
    assert_eq!(
        AgentEvent::Thinking { content: "".into() }.kind(),
        "thinking"
    );
    assert_eq!(
        AgentEvent::ToolUse {
            tool_name: "".into(),
            tool_use_id: "".into(),
            input: serde_json::Value::Null
        }
        .kind(),
        "tool_use"
    );
    assert_eq!(
        AgentEvent::ToolResult {
            tool_use_id: "".into(),
            success: true,
            output: "".into()
        }
        .kind(),
        "tool_result"
    );
    assert_eq!(
        AgentEvent::System {
            subtype: "".into(),
            metadata: serde_json::Value::Null
        }
        .kind(),
        "system"
    );
    assert_eq!(AgentEvent::Error { message: "".into() }.kind(), "error");
}

// ── ClaudeCodeAgent::parse_events ────────────────────────────────────────────

mod claude_parse {
    use bendclaw::kernel::tools::builtins::coding_agent::ClaudeCodeAgent;
    use bendclaw::kernel::tools::cli_agent::AgentEvent;
    use bendclaw::kernel::tools::cli_agent::CliAgent;

    fn parse(json: &str) -> Vec<AgentEvent> {
        let line: serde_json::Value = serde_json::from_str(json).unwrap();
        ClaudeCodeAgent.parse_events(&line)
    }

    #[test]
    fn system_init() {
        let events =
            parse(r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-4"}"#);
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::System { subtype, metadata } => {
                assert_eq!(subtype, "init");
                assert_eq!(metadata["session_id"], "s1");
                assert_eq!(metadata["model"], "claude-4");
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn assistant_text_block() {
        let events = parse(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello world"}]}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Text { content } if content == "hello world"));
    }

    #[test]
    fn assistant_thinking_block() {
        let events = parse(
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"let me think"}]}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::Thinking { content } if content == "let me think")
        );
    }

    #[test]
    fn assistant_tool_use_block() {
        let events = parse(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu_1","name":"Read","input":{"path":"/tmp"}}]}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(tool_use_id, "tu_1");
                assert_eq!(input["path"], "/tmp");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn assistant_mixed_blocks() {
        let events = parse(
            r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"analyzing"},
            {"type":"tool_use","id":"tu_2","name":"Bash","input":{"command":"ls"}}
        ]}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], AgentEvent::Text { .. }));
        assert!(matches!(&events[1], AgentEvent::ToolUse { .. }));
    }

    #[test]
    fn user_tool_result() {
        let events = parse(
            r#"{"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"tu_1","content":"file contents here"}
        ]}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolResult {
                tool_use_id,
                success,
                output,
            } => {
                assert_eq!(tool_use_id, "tu_1");
                assert!(success);
                assert_eq!(output, "file contents here");
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn user_tool_result_error() {
        let events = parse(
            r#"{"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"tu_1","is_error":true,"content":"not found"}
        ]}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolResult { success, .. } => assert!(!success),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn empty_content_produces_no_events() {
        assert!(parse(r#"{"type":"assistant","message":{"content":[]}}"#).is_empty());
    }

    #[test]
    fn unknown_type_produces_no_events() {
        assert!(parse(r#"{"type":"unknown_thing","data":123}"#).is_empty());
    }
}

// ── CodexAgent::parse_events ─────────────────────────────────────────────────

mod codex_parse {
    use bendclaw::kernel::tools::builtins::coding_agent::CodexAgent;
    use bendclaw::kernel::tools::cli_agent::AgentEvent;
    use bendclaw::kernel::tools::cli_agent::CliAgent;

    fn parse(json: &str) -> Vec<AgentEvent> {
        let line: serde_json::Value = serde_json::from_str(json).unwrap();
        CodexAgent.parse_events(&line)
    }

    #[test]
    fn item_started_command() {
        let events = parse(
            r#"{"type":"item.started","item":{"type":"commandExecution","id":"cmd_1","command":"ls -la"}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_use_id, "cmd_1");
                assert_eq!(input["command"], "ls -la");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn item_started_file_change() {
        let events = parse(
            r#"{"type":"item.started","item":{"type":"fileChange","id":"fc_1","filename":"src/main.rs"}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "FileEdit");
                assert_eq!(tool_use_id, "fc_1");
                assert_eq!(input["filename"], "src/main.rs");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn item_started_web_search() {
        let events = parse(
            r#"{"type":"item.started","item":{"type":"webSearch","id":"ws_1","query":"rust async"}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolUse {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "WebSearch");
                assert_eq!(input["query"], "rust async");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn item_completed_command_success() {
        let events = parse(
            r#"{"type":"item.completed","item":{"type":"commandExecution","id":"cmd_1","command":"echo hi","exit_code":0}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolResult {
                tool_use_id,
                success,
                output,
            } => {
                assert_eq!(tool_use_id, "cmd_1");
                assert!(success);
                assert!(output.contains("exit:0"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn item_completed_command_failure() {
        let events = parse(
            r#"{"type":"item.completed","item":{"type":"commandExecution","id":"cmd_1","command":"false","exit_code":1}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolResult { success, .. } => assert!(!success),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn item_completed_file_change() {
        let events = parse(
            r#"{"type":"item.completed","item":{"type":"fileChange","id":"fc_1","filename":"x.rs","status":"completed"}}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ToolResult {
                success, output, ..
            } => {
                assert!(success);
                assert!(output.contains("x.rs"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn item_completed_agent_message() {
        let events =
            parse(r#"{"type":"item.completed","item":{"type":"agent_message","text":"done!"}}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Text { content } if content == "done!"));
    }

    #[test]
    fn reasoning_delta() {
        let events = parse(r#"{"type":"item.reasoning.textDelta","text":"thinking..."}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Thinking { content } if content == "thinking..."));
    }

    #[test]
    fn agent_message_delta() {
        let events = parse(r#"{"type":"item.agentMessage.delta","text":"partial"}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Text { content } if content == "partial"));
    }

    #[test]
    fn plan_updated() {
        let events = parse(r#"{"type":"turn.plan.updated","steps":[{"title":"step 1"}]}"#);
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::System { subtype, metadata } => {
                assert_eq!(subtype, "plan_updated");
                assert!(metadata["steps"].is_array());
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn error_event() {
        let events = parse(r#"{"type":"error","message":"rate limited"}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Error { message } if message == "rate limited"));
    }

    #[test]
    fn unknown_type_produces_no_events() {
        assert!(parse(r#"{"type":"thread.tokenUsage.updated","usage":{}}"#).is_empty());
    }
}

// ── SSE mapping ──────────────────────────────────────────────────────────────

mod sse_mapping {
    use super::*;

    fn agent_event_sse_name(agent_event: AgentEvent) -> String {
        let (name, _) = map_agent_event("a", "s", "r", "tc_1", &agent_event);
        name
    }

    #[test]
    fn text_maps_to_tool_call_update() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::Text {
                content: "hi".into()
            }),
            "ToolCallUpdate"
        );
    }

    #[test]
    fn thinking_maps_to_tool_call_thinking() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::Thinking {
                content: "hmm".into()
            }),
            "ToolCallThinking"
        );
    }

    #[test]
    fn tool_use_maps_to_sub_tool_started() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::ToolUse {
                tool_name: "Read".into(),
                tool_use_id: "tu_1".into(),
                input: serde_json::Value::Null,
            }),
            "ToolCallSubToolStarted"
        );
    }

    #[test]
    fn tool_result_maps_to_sub_tool_completed() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::ToolResult {
                tool_use_id: "tu_1".into(),
                success: true,
                output: "ok".into(),
            }),
            "ToolCallSubToolCompleted"
        );
    }

    #[test]
    fn system_maps_to_tool_call_status() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::System {
                subtype: "init".into(),
                metadata: serde_json::Value::Null,
            }),
            "ToolCallStatus"
        );
    }

    #[test]
    fn error_maps_to_tool_call_error() {
        assert_eq!(
            agent_event_sse_name(AgentEvent::Error {
                message: "fail".into()
            }),
            "ToolCallError"
        );
    }

    #[test]
    fn tool_start_still_produces_sse() {
        let event = Event::ToolStart {
            tool_call_id: "tc_1".into(),
            name: "claude_code".into(),
            arguments: serde_json::json!({}),
        };
        assert!(map_event_to_sse("a", "s", "r", &event).is_some());
    }

    #[test]
    fn tool_end_still_produces_sse() {
        let event = Event::ToolEnd {
            tool_call_id: "tc_1".into(),
            name: "claude_code".into(),
            success: true,
            output: "done".into(),
            operation: bendclaw::kernel::OperationMeta::new(bendclaw::kernel::OpType::Execute),
        };
        assert!(map_event_to_sse("a", "s", "r", &event).is_some());
    }

    #[test]
    fn agent_event_payload_includes_tool_call_id() {
        let (_, payload) = map_agent_event("a", "s", "r", "tc_99", &AgentEvent::Text {
            content: "x".into(),
        });
        assert_eq!(payload["tool_call_id"], "tc_99");
        assert_eq!(payload["agent_event_kind"], "text");
    }
}
