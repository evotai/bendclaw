use anyhow::bail;
use anyhow::Result;
use bendclaw::kernel::run::ContentBlock;
use bendclaw::kernel::run::Delta;
use bendclaw::kernel::run::Event;
use bendclaw::kernel::run::Reason;
use bendclaw::kernel::run::Usage;
use bendclaw::kernel::tools::operation::OpType;
use bendclaw::kernel::tools::operation::OperationMeta;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::usage::TokenUsage;

// ── Reason ──

#[test]
fn reason_as_str() {
    assert_eq!(Reason::EndTurn.as_str(), "end_turn");
    assert_eq!(Reason::MaxIterations.as_str(), "max_iterations");
    assert_eq!(Reason::Timeout.as_str(), "timeout");
    assert_eq!(Reason::Aborted.as_str(), "aborted");
    assert_eq!(Reason::Error.as_str(), "error");
}

#[test]
fn reason_display() {
    assert_eq!(format!("{}", Reason::EndTurn), "end_turn");
    assert_eq!(format!("{}", Reason::Timeout), "timeout");
}

#[test]
fn reason_equality() {
    assert_eq!(Reason::EndTurn, Reason::EndTurn);
    assert_ne!(Reason::EndTurn, Reason::Timeout);
}

// ── ContentBlock ──

#[test]
fn content_block_text() -> Result<()> {
    let block = ContentBlock::text("hello");
    match block {
        ContentBlock::Text { text } => assert_eq!(text, "hello"),
        _ => bail!("expected Text"),
    }
    Ok(())
}

#[test]
fn content_block_thinking() -> Result<()> {
    let block = ContentBlock::thinking("reasoning...");
    match block {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "reasoning..."),
        _ => bail!("expected Thinking"),
    }
    Ok(())
}

// ── Usage ──

#[test]
fn usage_default_is_zero() {
    let u = Usage::default();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn usage_add_token_usage() {
    let mut u = Usage::default();
    let tu = TokenUsage::new(100, 50).with_cache(20, 10);
    u.add(&tu);
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 20);
    assert_eq!(u.cache_write_tokens, 10);
}

#[test]
fn usage_add_accumulates() {
    let mut u = Usage::default();
    u.add(&TokenUsage::new(100, 50));
    u.add(&TokenUsage::new(200, 80));
    assert_eq!(u.prompt_tokens, 300);
    assert_eq!(u.completion_tokens, 130);
    assert_eq!(u.total_tokens, 430);
}

#[test]
fn usage_merge() {
    let mut u1 = Usage::default();
    u1.add(&TokenUsage::new(100, 50).with_cache(10, 5));

    let mut u2 = Usage::default();
    u2.add(&TokenUsage::new(200, 80).with_cache(20, 10));

    u1.merge(&u2);
    assert_eq!(u1.prompt_tokens, 300);
    assert_eq!(u1.completion_tokens, 130);
    assert_eq!(u1.cache_read_tokens, 30);
    assert_eq!(u1.cache_write_tokens, 15);
}

#[test]
fn usage_cache_hit_rate() {
    let mut u = Usage::default();
    u.add(&TokenUsage::new(100, 50).with_cache(75, 0));
    assert!((u.cache_hit_rate() - 0.75).abs() < f64::EPSILON);
}

#[test]
fn usage_cache_hit_rate_zero_prompt() {
    let u = Usage::default();
    assert_eq!(u.cache_hit_rate(), 0.0);
}

// ── Delta::from_stream_event ──

#[test]
fn delta_from_content_delta() -> Result<()> {
    let event = StreamEvent::ContentDelta("hello".into());
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::Text { content } => assert_eq!(content, "hello"),
        _ => bail!("expected Text delta"),
    }
    Ok(())
}

#[test]
fn delta_from_thinking_delta() -> Result<()> {
    let event = StreamEvent::ThinkingDelta("step 1".into());
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::Thinking { content } => assert_eq!(content, "step 1"),
        _ => bail!("expected Thinking delta"),
    }
    Ok(())
}

#[test]
fn delta_from_tool_call_start() -> Result<()> {
    let event = StreamEvent::ToolCallStart {
        index: 0,
        id: "tc_001".into(),
        name: "shell".into(),
    };
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::ToolCallStart { index, id, name } => {
            assert_eq!(index, 0);
            assert_eq!(id, "tc_001");
            assert_eq!(name, "shell");
        }
        _ => bail!("expected ToolCallStart delta"),
    }
    Ok(())
}

#[test]
fn delta_from_tool_call_delta() -> Result<()> {
    let event = StreamEvent::ToolCallDelta {
        index: 0,
        json_chunk: r#"{"cmd"#.into(),
    };
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::ToolCallDelta { index, json_chunk } => {
            assert_eq!(index, 0);
            assert_eq!(json_chunk, r#"{"cmd"#);
        }
        _ => bail!("expected ToolCallDelta"),
    }
    Ok(())
}

#[test]
fn delta_from_tool_call_end() -> Result<()> {
    let event = StreamEvent::ToolCallEnd {
        index: 0,
        id: "tc_001".into(),
        name: "shell".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    };
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::ToolCallEnd {
            index,
            id,
            name,
            arguments,
        } => {
            assert_eq!(index, 0);
            assert_eq!(id, "tc_001");
            assert_eq!(name, "shell");
            assert_eq!(arguments, r#"{"command":"ls"}"#);
        }
        _ => bail!("expected ToolCallEnd"),
    }
    Ok(())
}

#[test]
fn delta_from_usage() -> Result<()> {
    let event = StreamEvent::Usage(TokenUsage::new(100, 50));
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::Usage(u) => {
            assert_eq!(u.prompt_tokens, 100);
            assert_eq!(u.completion_tokens, 50);
        }
        _ => bail!("expected Usage delta"),
    }
    Ok(())
}

#[test]
fn delta_from_done_contains_provider_and_model() -> Result<()> {
    let event = StreamEvent::Done {
        finish_reason: "stop".into(),
        provider: Some("openai".into()),
        model: Some("gpt-4.1-mini".into()),
    };
    let delta =
        Delta::from_stream_event(&event).ok_or_else(|| anyhow::anyhow!("expected Some delta"))?;
    match delta {
        Delta::Done {
            finish_reason,
            provider,
            model,
        } => {
            assert_eq!(finish_reason, "stop");
            assert_eq!(provider.as_deref(), Some("openai"));
            assert_eq!(model.as_deref(), Some("gpt-4.1-mini"));
        }
        _ => bail!("expected Done delta"),
    }
    Ok(())
}

#[test]
fn delta_from_error_returns_none() {
    let event = StreamEvent::Error("oops".into());
    assert!(Delta::from_stream_event(&event).is_none());
}

// ── Event serde ──

#[test]
fn event_start_serde_roundtrip() -> Result<()> {
    let e = Event::Start;
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    assert!(matches!(back, Event::Start));
    Ok(())
}

#[test]
fn event_end_serde_roundtrip() -> Result<()> {
    let e = Event::End {
        iterations: 3,
        stop_reason: "end_turn".into(),
        usage: Usage::default(),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::End {
            iterations,
            stop_reason,
            ..
        } => {
            assert_eq!(iterations, 3);
            assert_eq!(stop_reason, "end_turn");
        }
        _ => bail!("expected End"),
    }
    Ok(())
}

#[test]
fn event_aborted_serde_roundtrip() -> Result<()> {
    let e = Event::Aborted {
        reason: Reason::Timeout,
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::Aborted { reason } => assert_eq!(reason, Reason::Timeout),
        _ => bail!("expected Aborted"),
    }
    Ok(())
}

#[test]
fn event_turn_start_serde() -> Result<()> {
    let e = Event::TurnStart { iteration: 1 };
    let json = serde_json::to_string(&e)?;
    assert!(json.contains("TurnStart"));
    Ok(())
}

#[test]
fn event_compaction_done_serde() -> Result<()> {
    let e = Event::CompactionDone {
        messages_before: 100,
        messages_after: 10,
        summary_len: 500,
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::CompactionDone {
            messages_before,
            messages_after,
            summary_len,
        } => {
            assert_eq!(messages_before, 100);
            assert_eq!(messages_after, 10);
            assert_eq!(summary_len, 500);
        }
        _ => bail!("expected CompactionDone"),
    }
    Ok(())
}

#[test]
fn event_error_serde() -> Result<()> {
    let e = Event::Error {
        message: "something broke".into(),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::Error { message } => assert_eq!(message, "something broke"),
        _ => bail!("expected Error"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_text() -> Result<()> {
    let d = Delta::Text {
        content: "hi".into(),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::Text { content } => assert_eq!(content, "hi"),
        _ => bail!("expected Text"),
    }
    Ok(())
}

// ── Delta serde – remaining variants ─────────────────────────────────────────

#[test]
fn delta_serde_roundtrip_thinking() -> Result<()> {
    let d = Delta::Thinking {
        content: "ponder".into(),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::Thinking { content } => assert_eq!(content, "ponder"),
        _ => bail!("expected Thinking"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_tool_call_start() -> Result<()> {
    let d = Delta::ToolCallStart {
        index: 2,
        id: "tc_x".into(),
        name: "grep".into(),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::ToolCallStart { index, id, name } => {
            assert_eq!(index, 2);
            assert_eq!(id, "tc_x");
            assert_eq!(name, "grep");
        }
        _ => bail!("expected ToolCallStart"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_tool_call_delta() -> Result<()> {
    let d = Delta::ToolCallDelta {
        index: 1,
        json_chunk: r#"{"k":"#.into(),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::ToolCallDelta { index, json_chunk } => {
            assert_eq!(index, 1);
            assert_eq!(json_chunk, r#"{"k":"#);
        }
        _ => bail!("expected ToolCallDelta"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_tool_call_end() -> Result<()> {
    let d = Delta::ToolCallEnd {
        index: 0,
        id: "tc_1".into(),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::ToolCallEnd {
            index,
            id,
            name,
            arguments,
        } => {
            assert_eq!(index, 0);
            assert_eq!(id, "tc_1");
            assert_eq!(name, "shell");
            assert_eq!(arguments, r#"{"cmd":"ls"}"#);
        }
        _ => bail!("expected ToolCallEnd"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_done() -> Result<()> {
    let d = Delta::Done {
        finish_reason: "stop".into(),
        provider: Some("anthropic".into()),
        model: Some("claude-3".into()),
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::Done {
            finish_reason,
            provider,
            model,
        } => {
            assert_eq!(finish_reason, "stop");
            assert_eq!(provider.as_deref(), Some("anthropic"));
            assert_eq!(model.as_deref(), Some("claude-3"));
        }
        _ => bail!("expected Done"),
    }
    Ok(())
}

#[test]
fn delta_serde_roundtrip_done_no_provider() -> Result<()> {
    let d = Delta::Done {
        finish_reason: "length".into(),
        provider: None,
        model: None,
    };
    let json = serde_json::to_string(&d)?;
    let back: Delta = serde_json::from_str(&json)?;
    match back {
        Delta::Done {
            finish_reason,
            provider,
            model,
        } => {
            assert_eq!(finish_reason, "length");
            assert!(provider.is_none());
            assert!(model.is_none());
        }
        _ => bail!("expected Done"),
    }
    Ok(())
}

// ── Event serde – remaining variants ─────────────────────────────────────────

#[test]
fn event_turn_end_serde() -> Result<()> {
    let e = Event::TurnEnd { iteration: 5 };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::TurnEnd { iteration } => assert_eq!(iteration, 5),
        _ => bail!("expected TurnEnd"),
    }
    Ok(())
}

#[test]
fn event_reason_start_serde() -> Result<()> {
    let e = Event::ReasonStart;
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    assert!(matches!(back, Event::ReasonStart));
    Ok(())
}

#[test]
fn event_reason_end_serde() -> Result<()> {
    let e = Event::ReasonEnd {
        finish_reason: "stop".into(),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::ReasonEnd { finish_reason } => assert_eq!(finish_reason, "stop"),
        _ => bail!("expected ReasonEnd"),
    }
    Ok(())
}

#[test]
fn event_reason_error_serde() -> Result<()> {
    let e = Event::ReasonError {
        error: "timeout".into(),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::ReasonError { error } => assert_eq!(error, "timeout"),
        _ => bail!("expected ReasonError"),
    }
    Ok(())
}

#[test]
fn event_tool_start_serde() -> Result<()> {
    let e = Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        arguments: serde_json::json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::ToolStart {
            tool_call_id,
            name,
            arguments,
        } => {
            assert_eq!(tool_call_id, "tc_1");
            assert_eq!(name, "shell");
            assert_eq!(arguments["command"], "ls");
        }
        _ => bail!("expected ToolStart"),
    }
    Ok(())
}

#[test]
fn event_tool_update_serde() -> Result<()> {
    let e = Event::ToolUpdate {
        tool_call_id: "tc_1".into(),
        output: "partial".into(),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::ToolUpdate {
            tool_call_id,
            output,
        } => {
            assert_eq!(tool_call_id, "tc_1");
            assert_eq!(output, "partial");
        }
        _ => bail!("expected ToolUpdate"),
    }
    Ok(())
}

#[test]
fn event_tool_end_serde() -> Result<()> {
    let e = Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        success: true,
        output: "ok".into(),
        operation: OperationMeta::new(OpType::Execute),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::ToolEnd {
            tool_call_id,
            name,
            success,
            output,
            ..
        } => {
            assert_eq!(tool_call_id, "tc_1");
            assert_eq!(name, "shell");
            assert!(success);
            assert_eq!(output, "ok");
        }
        _ => bail!("expected ToolEnd"),
    }
    Ok(())
}

#[test]
fn event_checkpoint_done_serde() -> Result<()> {
    let e = Event::CheckpointDone {
        prompt_tokens: 500,
        completion_tokens: 200,
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::CheckpointDone {
            prompt_tokens,
            completion_tokens,
        } => {
            assert_eq!(prompt_tokens, 500);
            assert_eq!(completion_tokens, 200);
        }
        _ => bail!("expected CheckpointDone"),
    }
    Ok(())
}

#[test]
fn event_app_data_serde() -> Result<()> {
    let e = Event::AppData(serde_json::json!({"step": 1, "status": "running"}));
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::AppData(v) => {
            assert_eq!(v["step"], 1);
            assert_eq!(v["status"], "running");
        }
        _ => bail!("expected AppData"),
    }
    Ok(())
}

#[test]
fn event_audit_serde() -> Result<()> {
    let e = Event::Audit {
        name: "llm.request".into(),
        payload: serde_json::json!({"model": "gpt-4.1-mini"}),
    };
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::Audit { name, payload } => {
            assert_eq!(name, "llm.request");
            assert_eq!(payload["model"], "gpt-4.1-mini");
        }
        _ => bail!("expected Audit"),
    }
    Ok(())
}
