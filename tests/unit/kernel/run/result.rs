use anyhow::bail;
use anyhow::Result;
use bendclaw::kernel::run::ContentBlock;
use bendclaw::kernel::run::Reason;
use bendclaw::kernel::run::Usage;
use bendclaw::llm::usage::TokenUsage;

#[test]
fn content_block_serde_roundtrip() -> Result<()> {
    let b = ContentBlock::text("test");
    let json = serde_json::to_string(&b)?;
    let back: ContentBlock = serde_json::from_str(&json)?;
    match back {
        ContentBlock::Text { text } => assert_eq!(text, "test"),
        _ => bail!("expected Text"),
    }
    Ok(())
}

#[test]
fn reason_serde_roundtrip() -> Result<()> {
    for reason in [
        Reason::EndTurn,
        Reason::MaxIterations,
        Reason::Timeout,
        Reason::Aborted,
        Reason::Error,
    ] {
        let json = serde_json::to_string(&reason)?;
        let back: Reason = serde_json::from_str(&json)?;
        assert_eq!(back, reason);
    }
    Ok(())
}

#[test]
fn usage_default_checks_all_fields() {
    let u = Usage::default();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.reasoning_tokens, 0);
    assert_eq!(u.total_tokens, 0);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
    assert_eq!(u.ttft_ms, 0);
}

#[test]
fn usage_add_token_usage_raw() {
    let mut u = Usage::default();
    let tu = TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
        cache_read_tokens: 5,
        cache_write_tokens: 3,
    };
    u.add(&tu);
    assert_eq!(u.prompt_tokens, 10);
    assert_eq!(u.completion_tokens, 20);
    assert_eq!(u.total_tokens, 30);
    assert_eq!(u.cache_read_tokens, 5);
    assert_eq!(u.cache_write_tokens, 3);
}

#[test]
fn usage_merge_raw() {
    let mut a = Usage {
        prompt_tokens: 10,
        completion_tokens: 20,
        reasoning_tokens: 0,
        total_tokens: 30,
        cache_read_tokens: 5,
        cache_write_tokens: 2,
        ttft_ms: 0,
    };
    let b = Usage {
        prompt_tokens: 5,
        completion_tokens: 10,
        reasoning_tokens: 0,
        total_tokens: 15,
        cache_read_tokens: 3,
        cache_write_tokens: 1,
        ttft_ms: 0,
    };
    a.merge(&b);
    assert_eq!(a.prompt_tokens, 15);
    assert_eq!(a.completion_tokens, 30);
    assert_eq!(a.total_tokens, 45);
}

#[test]
fn usage_cache_hit_rate_with_reads() {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 100,
        cache_read_tokens: 50,
        cache_write_tokens: 0,
        ttft_ms: 0,
    };
    assert!((u.cache_hit_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn result_text_extracts_text_blocks() {
    let r = bendclaw::kernel::run::Result {
        content: vec![
            ContentBlock::thinking("hmm"),
            ContentBlock::text("hello"),
            ContentBlock::text(" world"),
        ],
        iterations: 1,
        usage: Usage::default(),
        stop_reason: Reason::EndTurn,
        checkpoint: None,
        messages: vec![],
    };
    assert_eq!(r.text(), "hello world");
}

#[test]
fn result_aborted() {
    let r = bendclaw::kernel::run::Result::aborted();
    assert!(r.content.is_empty());
    assert_eq!(r.iterations, 0);
    assert_eq!(r.stop_reason, Reason::Aborted);
    assert!(r.text().is_empty());
}

#[test]
fn content_block_thinking_serde_roundtrip() -> Result<()> {
    let b = ContentBlock::thinking("deep thought");
    let json = serde_json::to_string(&b)?;
    let back: ContentBlock = serde_json::from_str(&json)?;
    match back {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "deep thought"),
        _ => bail!("expected Thinking"),
    }
    Ok(())
}

#[test]
fn content_block_constructors_accept_string_and_str() -> Result<()> {
    match ContentBlock::text("hello") {
        ContentBlock::Text { text } => assert_eq!(text, "hello"),
        _ => bail!("expected Text"),
    }
    match ContentBlock::text(String::from("world")) {
        ContentBlock::Text { text } => assert_eq!(text, "world"),
        _ => bail!("expected Text"),
    }
    match ContentBlock::thinking("reason") {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "reason"),
        _ => bail!("expected Thinking"),
    }
    match ContentBlock::thinking(String::from("logic")) {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "logic"),
        _ => bail!("expected Thinking"),
    }
    Ok(())
}

#[test]
fn reason_as_str_all_variants() {
    assert_eq!(Reason::EndTurn.as_str(), "end_turn");
    assert_eq!(Reason::MaxIterations.as_str(), "max_iterations");
    assert_eq!(Reason::Timeout.as_str(), "timeout");
    assert_eq!(Reason::Aborted.as_str(), "aborted");
    assert_eq!(Reason::Error.as_str(), "error");
}

#[test]
fn reason_display_all_variants() {
    assert_eq!(Reason::EndTurn.to_string(), "end_turn");
    assert_eq!(Reason::MaxIterations.to_string(), "max_iterations");
    assert_eq!(Reason::Timeout.to_string(), "timeout");
    assert_eq!(Reason::Aborted.to_string(), "aborted");
    assert_eq!(Reason::Error.to_string(), "error");
}

#[test]
fn usage_merge_ttft_keeps_first_nonzero() {
    let mut a = Usage::default();
    let b = Usage {
        ttft_ms: 42,
        ..Usage::default()
    };
    a.merge(&b);
    assert_eq!(a.ttft_ms, 42);

    let c = Usage {
        ttft_ms: 99,
        ..Usage::default()
    };
    a.merge(&c);
    assert_eq!(a.ttft_ms, 42);
}

#[test]
fn usage_merge_ttft_ignores_zero_other() {
    let mut a = Usage::default();
    let b = Usage {
        ttft_ms: 0,
        ..Usage::default()
    };
    a.merge(&b);
    assert_eq!(a.ttft_ms, 0);
}

#[test]
fn usage_merge_reasoning_tokens() {
    let mut a = Usage {
        reasoning_tokens: 10,
        ..Usage::default()
    };
    let b = Usage {
        reasoning_tokens: 25,
        ..Usage::default()
    };
    a.merge(&b);
    assert_eq!(a.reasoning_tokens, 35);
}

#[test]
fn usage_add_accumulates_multiple_calls() {
    let mut u = Usage::default();
    let tu = TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        cache_read_tokens: 2,
        cache_write_tokens: 1,
    };
    u.add(&tu);
    u.add(&tu);
    assert_eq!(u.prompt_tokens, 20);
    assert_eq!(u.completion_tokens, 10);
    assert_eq!(u.total_tokens, 30);
    assert_eq!(u.cache_read_tokens, 4);
    assert_eq!(u.cache_write_tokens, 2);
    assert_eq!(u.reasoning_tokens, 0);
    assert_eq!(u.ttft_ms, 0);
}

#[test]
fn usage_cache_hit_rate_zero_prompt_tokens() {
    let u = Usage::default();
    assert!((u.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn usage_cache_hit_rate_full_cache() {
    let u = Usage {
        prompt_tokens: 200,
        cache_read_tokens: 200,
        ..Usage::default()
    };
    assert!((u.cache_hit_rate() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn result_text_empty_content() {
    let r = bendclaw::kernel::run::Result {
        content: vec![],
        iterations: 0,
        usage: Usage::default(),
        stop_reason: Reason::EndTurn,
        checkpoint: None,
        messages: vec![],
    };
    assert_eq!(r.text(), "");
}

#[test]
fn result_text_thinking_only_returns_empty() {
    let r = bendclaw::kernel::run::Result {
        content: vec![
            ContentBlock::thinking("step 1"),
            ContentBlock::thinking("step 2"),
        ],
        iterations: 1,
        usage: Usage::default(),
        stop_reason: Reason::EndTurn,
        checkpoint: None,
        messages: vec![],
    };
    assert_eq!(r.text(), "");
}

#[test]
fn usage_serde_roundtrip() -> Result<()> {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 30,
        total_tokens: 180,
        cache_read_tokens: 40,
        cache_write_tokens: 10,
        ttft_ms: 123,
    };
    let json = serde_json::to_string(&u)?;
    let back: Usage = serde_json::from_str(&json)?;
    assert_eq!(back.prompt_tokens, 100);
    assert_eq!(back.completion_tokens, 50);
    assert_eq!(back.reasoning_tokens, 30);
    assert_eq!(back.total_tokens, 180);
    assert_eq!(back.cache_read_tokens, 40);
    assert_eq!(back.cache_write_tokens, 10);
    assert_eq!(back.ttft_ms, 123);
    Ok(())
}

#[test]
fn result_serde_roundtrip() -> Result<()> {
    let r = bendclaw::kernel::run::Result {
        content: vec![ContentBlock::thinking("hmm"), ContentBlock::text("answer")],
        iterations: 3,
        usage: Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            reasoning_tokens: 5,
            total_tokens: 35,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            ttft_ms: 50,
        },
        stop_reason: Reason::MaxIterations,
        checkpoint: None,
        messages: vec![],
    };
    let json = serde_json::to_string(&r)?;
    let back: bendclaw::kernel::run::Result = serde_json::from_str(&json)?;
    assert_eq!(back.text(), "answer");
    assert_eq!(back.iterations, 3);
    assert_eq!(back.stop_reason, Reason::MaxIterations);
    assert_eq!(back.usage.prompt_tokens, 10);
    assert_eq!(back.usage.ttft_ms, 50);
    Ok(())
}
