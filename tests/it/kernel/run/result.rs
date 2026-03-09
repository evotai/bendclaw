use bendclaw::kernel::run::ContentBlock;
use bendclaw::kernel::run::Reason;
use bendclaw::kernel::run::Usage;
use bendclaw::llm::usage::TokenUsage;

#[test]
fn content_block_serde_roundtrip() {
    let b = ContentBlock::text("test");
    let json = serde_json::to_string(&b).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::Text { text } => assert_eq!(text, "test"),
        _ => panic!("expected Text"),
    }
}

#[test]
fn reason_serde_roundtrip() {
    for reason in [
        Reason::EndTurn,
        Reason::MaxIterations,
        Reason::Timeout,
        Reason::Aborted,
        Reason::Error,
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        let back: Reason = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reason);
    }
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
fn content_block_thinking_serde_roundtrip() {
    let b = ContentBlock::thinking("deep thought");
    let json = serde_json::to_string(&b).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "deep thought"),
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn content_block_constructors_accept_string_and_str() {
    // &str
    match ContentBlock::text("hello") {
        ContentBlock::Text { text } => assert_eq!(text, "hello"),
        _ => panic!("expected Text"),
    }
    // String
    match ContentBlock::text(String::from("world")) {
        ContentBlock::Text { text } => assert_eq!(text, "world"),
        _ => panic!("expected Text"),
    }
    // &str
    match ContentBlock::thinking("reason") {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "reason"),
        _ => panic!("expected Thinking"),
    }
    // String
    match ContentBlock::thinking(String::from("logic")) {
        ContentBlock::Thinking { thinking } => assert_eq!(thinking, "logic"),
        _ => panic!("expected Thinking"),
    }
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
    // First merge: ttft_ms picked up from other.
    let mut a = Usage::default();
    let b = Usage {
        ttft_ms: 42,
        ..Usage::default()
    };
    a.merge(&b);
    assert_eq!(a.ttft_ms, 42);

    // Second merge: ttft_ms already set, should NOT be overwritten.
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
    // reasoning_tokens and ttft_ms are untouched by add.
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
        messages: vec![],
    };
    assert_eq!(r.text(), "");
}

#[test]
fn usage_serde_roundtrip() {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 30,
        total_tokens: 180,
        cache_read_tokens: 40,
        cache_write_tokens: 10,
        ttft_ms: 123,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.prompt_tokens, 100);
    assert_eq!(back.completion_tokens, 50);
    assert_eq!(back.reasoning_tokens, 30);
    assert_eq!(back.total_tokens, 180);
    assert_eq!(back.cache_read_tokens, 40);
    assert_eq!(back.cache_write_tokens, 10);
    assert_eq!(back.ttft_ms, 123);
}

#[test]
fn result_serde_roundtrip() {
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
        messages: vec![],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: bendclaw::kernel::run::Result = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text(), "answer");
    assert_eq!(back.iterations, 3);
    assert_eq!(back.stop_reason, Reason::MaxIterations);
    assert_eq!(back.usage.prompt_tokens, 10);
    assert_eq!(back.usage.ttft_ms, 50);
}

// ── ModelRole ──

use bendclaw::kernel::run::usage::CostSummary;
use bendclaw::kernel::run::usage::ModelRole;
use bendclaw::kernel::run::usage::UsageScope;

#[test]
fn model_role_default_is_reasoning() {
    assert_eq!(ModelRole::default(), ModelRole::Reasoning);
}

#[test]
fn model_role_as_str_all_variants() {
    assert_eq!(ModelRole::Reasoning.as_str(), "reasoning");
    assert_eq!(ModelRole::Compaction.as_str(), "compaction");
    assert_eq!(ModelRole::Checkpoint.as_str(), "checkpoint");
}

#[test]
fn model_role_display_all_variants() {
    assert_eq!(ModelRole::Reasoning.to_string(), "reasoning");
    assert_eq!(ModelRole::Compaction.to_string(), "compaction");
    assert_eq!(ModelRole::Checkpoint.to_string(), "checkpoint");
}

#[test]
fn model_role_serde_roundtrip_reasoning() {
    let json = serde_json::to_string(&ModelRole::Reasoning).unwrap();
    let back: ModelRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ModelRole::Reasoning);
}

#[test]
fn model_role_serde_roundtrip_compaction() {
    let json = serde_json::to_string(&ModelRole::Compaction).unwrap();
    let back: ModelRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ModelRole::Compaction);
}

#[test]
fn model_role_serde_roundtrip_checkpoint() {
    let json = serde_json::to_string(&ModelRole::Checkpoint).unwrap();
    let back: ModelRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ModelRole::Checkpoint);
}

#[test]
fn model_role_serde_lowercase_reasoning() {
    let json = serde_json::to_string(&ModelRole::Reasoning).unwrap();
    assert_eq!(json, "\"reasoning\"");
}

#[test]
fn model_role_serde_lowercase_compaction() {
    let json = serde_json::to_string(&ModelRole::Compaction).unwrap();
    assert_eq!(json, "\"compaction\"");
}

#[test]
fn model_role_serde_lowercase_checkpoint() {
    let json = serde_json::to_string(&ModelRole::Checkpoint).unwrap();
    assert_eq!(json, "\"checkpoint\"");
}

#[test]
fn model_role_clone_and_eq() {
    let r = ModelRole::Compaction;
    let c = r;
    assert_eq!(r, c);
}

// ── CostSummary ──

#[test]
fn cost_summary_default_all_zeros() {
    let s = CostSummary::default();
    assert_eq!(s.total_prompt_tokens, 0);
    assert_eq!(s.total_completion_tokens, 0);
    assert_eq!(s.total_reasoning_tokens, 0);
    assert_eq!(s.total_tokens, 0);
    assert_eq!(s.total_cost, 0.0);
    assert_eq!(s.record_count, 0);
    assert_eq!(s.total_cache_read_tokens, 0);
    assert_eq!(s.total_cache_write_tokens, 0);
}

#[test]
fn cost_summary_serde_roundtrip() {
    let s = CostSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    let back: CostSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_prompt_tokens, 0);
    assert_eq!(back.record_count, 0);
}

#[test]
fn cost_summary_serde_with_values() {
    let s = CostSummary {
        total_prompt_tokens: 1000,
        total_completion_tokens: 500,
        total_reasoning_tokens: 200,
        total_tokens: 1700,
        total_cost: 0.042,
        record_count: 7,
        total_cache_read_tokens: 300,
        total_cache_write_tokens: 100,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: CostSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_prompt_tokens, 1000);
    assert_eq!(back.total_completion_tokens, 500);
    assert_eq!(back.total_reasoning_tokens, 200);
    assert_eq!(back.total_tokens, 1700);
    assert_eq!(back.record_count, 7);
    assert_eq!(back.total_cache_read_tokens, 300);
    assert_eq!(back.total_cache_write_tokens, 100);
    assert!((back.total_cost - 0.042).abs() < f64::EPSILON);
}

// ── UsageScope ──

#[test]
fn usage_scope_user_variant() {
    let scope = UsageScope::User {
        user_id: "u1".into(),
    };
    match scope {
        UsageScope::User { user_id } => assert_eq!(user_id, "u1"),
        _ => panic!("expected User variant"),
    }
}

#[test]
fn usage_scope_agent_total_variant() {
    let scope = UsageScope::AgentTotal {
        agent_id: "a1".into(),
    };
    match scope {
        UsageScope::AgentTotal { agent_id } => assert_eq!(agent_id, "a1"),
        _ => panic!("expected AgentTotal variant"),
    }
}

#[test]
fn usage_scope_agent_daily_variant() {
    let scope = UsageScope::AgentDaily {
        agent_id: "a2".into(),
        day: "2026-03-05".into(),
    };
    match scope {
        UsageScope::AgentDaily { agent_id, day } => {
            assert_eq!(agent_id, "a2");
            assert_eq!(day, "2026-03-05");
        }
        _ => panic!("expected AgentDaily variant"),
    }
}

// ── UsageEvent ──

#[test]
fn usage_event_fields_accessible() {
    use bendclaw::kernel::run::usage::UsageEvent;
    let ev = UsageEvent {
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        session_id: "sess-1".into(),
        run_id: "run-1".into(),
        provider: "openai".into(),
        model: "gpt-4o".into(),
        model_role: ModelRole::Reasoning,
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 10,
        cache_read_tokens: 20,
        cache_write_tokens: 5,
        ttft_ms: 300,
        cost: 0.0042,
    };
    assert_eq!(ev.agent_id, "agent-1");
    assert_eq!(ev.user_id, "user-1");
    assert_eq!(ev.session_id, "sess-1");
    assert_eq!(ev.run_id, "run-1");
    assert_eq!(ev.provider, "openai");
    assert_eq!(ev.model, "gpt-4o");
    assert_eq!(ev.model_role, ModelRole::Reasoning);
    assert_eq!(ev.prompt_tokens, 100);
    assert_eq!(ev.completion_tokens, 50);
    assert_eq!(ev.reasoning_tokens, 10);
    assert_eq!(ev.cache_read_tokens, 20);
    assert_eq!(ev.cache_write_tokens, 5);
    assert_eq!(ev.ttft_ms, 300);
    assert!((ev.cost - 0.0042).abs() < f64::EPSILON);
}

#[test]
fn usage_event_clone() {
    use bendclaw::kernel::run::usage::UsageEvent;
    let ev = UsageEvent {
        agent_id: "agent-2".into(),
        user_id: "user-2".into(),
        session_id: "sess-2".into(),
        run_id: "run-2".into(),
        provider: "anthropic".into(),
        model: "claude-3".into(),
        model_role: ModelRole::Compaction,
        prompt_tokens: 200,
        completion_tokens: 80,
        reasoning_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        ttft_ms: 150,
        cost: 0.01,
    };
    let cloned = ev.clone();
    assert_eq!(cloned.agent_id, ev.agent_id);
    assert_eq!(cloned.model_role, ev.model_role);
    assert_eq!(cloned.prompt_tokens, ev.prompt_tokens);
    assert!((cloned.cost - ev.cost).abs() < f64::EPSILON);
}
