use bendclaw::agent::RunEventPayload;
use bendclaw::cli::repl::transcript_log::format_event;

#[test]
fn format_tool_started() {
    let payload = RunEventPayload::ToolStarted {
        tool_call_id: "tc1".into(),
        tool_name: "read_file".into(),
        args: serde_json::json!({"path": "src/main.rs"}),
        preview_command: None,
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "[read_file call]");
    assert!(lines[1].contains("path: src/main.rs"));
}

#[test]
fn format_tool_finished_ok() {
    let payload = RunEventPayload::ToolFinished {
        tool_call_id: "tc1".into(),
        tool_name: "read_file".into(),
        content: "file contents here".into(),
        is_error: false,
        details: serde_json::Value::Null,
        result_tokens: 5,
        duration_ms: 120,
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "[read_file completed]");
    assert!(lines[1].contains("file contents here"));
}

#[test]
fn format_tool_finished_error() {
    let payload = RunEventPayload::ToolFinished {
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        content: "".into(),
        is_error: true,
        details: serde_json::Value::Null,
        result_tokens: 0,
        duration_ms: 0,
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "[bash failed]");
    assert!(lines[1].contains("tool returned an error"));
}

#[test]
fn format_error_event() {
    let payload = RunEventPayload::Error {
        message: "something broke".into(),
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "[error] something broke");
}

#[test]
fn format_delta_is_empty() {
    let payload = RunEventPayload::AssistantDelta {
        delta: Some("hello".into()),
        thinking_delta: None,
    };
    assert!(format_event(&payload).is_empty());
}

#[test]
fn format_assistant_text() {
    let payload = RunEventPayload::AssistantCompleted {
        content: vec![bendclaw::agent::AssistantBlock::Text {
            text: "Hello world".into(),
        }],
        usage: None,
        stop_reason: "end_turn".into(),
        error_message: None,
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "Hello world");
}

#[test]
fn format_assistant_error_stop_reason() {
    let payload = RunEventPayload::AssistantCompleted {
        content: vec![],
        usage: None,
        stop_reason: "error".into(),
        error_message: Some("rate limited".into()),
    };
    let lines = format_event(&payload);
    assert!(lines.iter().any(|l| l.contains("[error] rate limited")));
}

#[test]
fn format_run_finished() {
    let payload = RunEventPayload::RunFinished {
        text: "done".into(),
        usage: bendclaw::agent::UsageSummary {
            input: 100,
            output: 50,
            cache_read: 0,
            cache_write: 0,
        },
        turn_count: 2,
        duration_ms: 1500,
        transcript_count: 4,
    };
    let lines = format_event(&payload);
    assert_eq!(lines[0], "---");
    assert!(lines[1].contains("1.5s"));
    assert!(lines[1].contains("turns 2"));
}

#[test]
fn format_llm_call_started() {
    let payload = RunEventPayload::LlmCallStarted {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        system_prompt: "you are helpful".into(),
        messages: vec![serde_json::json!({"role": "user", "content": "hi"})],
        tools: vec![],
        message_count: 1,
        message_bytes: 30,
        system_prompt_tokens: 10,
    };
    let lines = format_event(&payload);
    assert!(lines[0].contains("[llm call]"));
    assert!(lines[0].contains("claude-3"));
    assert!(lines[0].contains("turn 1"));
}

#[test]
fn format_llm_call_completed() {
    let payload = RunEventPayload::LlmCallCompleted {
        turn: 1,
        attempt: 0,
        usage: bendclaw::agent::UsageSummary {
            input: 200,
            output: 80,
            cache_read: 0,
            cache_write: 0,
        },
        cache_read: 50,
        cache_write: 10,
        error: None,
        metrics: None,
    };
    let lines = format_event(&payload);
    assert!(lines[0].contains("[llm completed]"));
    assert!(lines[0].contains("200 input"));
    assert!(lines[0].contains("80 output"));
    assert!(lines[0].contains("cache r:50 w:10"));
}

#[test]
fn format_timestamp_valid_rfc3339() {
    use bendclaw::cli::repl::transcript_log::format_timestamp;
    let ts = format_timestamp("2026-04-07T10:30:45+00:00");
    // Should produce HH:MM:SS in local time
    assert_eq!(ts.len(), 8);
    assert!(ts.contains(':'));
}

#[test]
fn format_timestamp_invalid_falls_back() {
    use bendclaw::cli::repl::transcript_log::format_timestamp;
    let ts = format_timestamp("not-a-date");
    assert_eq!(ts, "not-a-date");
}

#[test]
fn write_event_includes_timestamp() {
    use bendclaw::agent::RunEvent;
    use bendclaw::agent::RunEventPayload;
    use bendclaw::cli::repl::transcript_log::TranscriptLog;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.log");
    let log = TranscriptLog::from_path(path.clone());

    let event = RunEvent::new("run1".into(), "sess1".into(), 0, RunEventPayload::Error {
        message: "boom".into(),
    });
    log.write_event(&event);

    let content = std::fs::read_to_string(&path).unwrap();
    // First line should be [HH:MM:SS] [error] boom
    let first_line = content.lines().next().unwrap();
    assert!(first_line.starts_with('['));
    assert!(first_line.contains("[error] boom"));
}

#[test]
fn write_user_prompt_includes_timestamp() {
    use bendclaw::cli::repl::transcript_log::TranscriptLog;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.log");
    let log = TranscriptLog::from_path(path.clone());

    log.write_user_prompt("hello world");

    let content = std::fs::read_to_string(&path).unwrap();
    let first_line = content.lines().next().unwrap();
    assert!(first_line.starts_with('['));
    assert!(first_line.contains("> hello world"));
}
