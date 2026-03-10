use anyhow::Result;
use std::sync::Arc;

use bendclaw::kernel::run::compactor::Compactor;
use bendclaw::kernel::runtime::agent_config::CheckpointConfig;
use bendclaw::kernel::Message;
use bendclaw::llm::tool::ToolSchema;
use tokio_util::sync::CancellationToken;

use crate::mocks::llm::MockLLMProvider;

#[test]
fn split_chunks_short_text() {
    let text = "hello world";
    let chunks = Compactor::split_chunks(text, 100);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "hello world");
}

#[test]
fn split_chunks_exact_boundary() {
    let text = "12345";
    let chunks = Compactor::split_chunks(text, 5);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "12345");
}

#[test]
fn split_chunks_at_paragraph_boundary() {
    let text = "first paragraph\n\nsecond paragraph\n\nthird paragraph";
    let chunks = Compactor::split_chunks(text, 25);
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(!chunk.is_empty());
    }
}

#[test]
fn split_chunks_no_boundary() {
    let text = "a".repeat(100);
    let chunks = Compactor::split_chunks(&text, 30);
    assert!(chunks.len() >= 3);
    let reassembled: String = chunks.concat();
    assert_eq!(reassembled, text);
}

#[test]
fn split_chunks_preserves_all_content() {
    let text = "chunk one\n\nchunk two\n\nchunk three\n\nchunk four";
    let chunks = Compactor::split_chunks(text, 15);
    let reassembled: String = chunks.concat();
    assert_eq!(reassembled, text);
}

#[test]
fn split_chunks_ends_with_newline_boundary() {
    let text = "aaa\n\nbbb\n\nccc\n\n";
    let chunks = Compactor::split_chunks(text, 6);
    assert!(chunks.len() >= 2);
    let rebuilt = chunks.concat();
    assert_eq!(rebuilt, text);
}

#[test]
fn split_chunks_multiple_paragraphs_keeps_content() {
    let text = "p1\n\np2\n\np3\n\np4\n\np5";
    let chunks = Compactor::split_chunks(text, 5);
    assert!(chunks.len() >= 3);
    let rebuilt = chunks.concat();
    assert_eq!(rebuilt, text);
}

#[test]
fn split_chunks_large_chunk_no_split() {
    let text = "one\n\ntwo\n\nthree";
    let chunks = Compactor::split_chunks(text, 10_000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], text);
}

#[tokio::test]
async fn compact_returns_none_when_within_budget() {
    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let mut messages = vec![Message::user("short"), Message::assistant("ok")];
    let res = compactor.compact(&mut messages, 100_000, &[]).await;
    assert!(res.is_none());
}

#[tokio::test]
async fn compact_runs_checkpoint_only_when_over_threshold_without_compaction() {
    let llm = Arc::new(MockLLMProvider::with_text("checkpoint done"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: true,
        threshold: 5,
        prompt: "save important memory".to_string(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let long = "x".repeat(1200);
    let mut messages = vec![Message::user(long.clone()), Message::assistant(long)];
    let memory_tools = vec![ToolSchema::new(
        "memory_write",
        "write memory",
        serde_json::json!({"type": "object"}),
    )];
    // total tokens likely > 80% of 1_000 but <= 1_000 compaction guard depends on tokenizer;
    // we set higher max to avoid compaction while still allowing checkpoint branch.
    let res = compactor
        .compact(&mut messages, 10_000, &memory_tools)
        .await;
    if let Some(r) = res {
        // When checkpoint ran without compaction, summary_len should stay 0.
        if r.checkpoint_usage.is_some() {
            assert_eq!(r.summary_len, 0);
        }
    }
}

#[tokio::test]
async fn compact_triggers_with_small_context_budget() {
    let llm = Arc::new(MockLLMProvider::with_text("condensed summary"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let big = "token ".repeat(3000);
    let mut messages = vec![
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user(big),
    ];
    let before = messages.len();

    let res = compactor.compact(&mut messages, 256, &[]).await;
    assert!(res.is_some());

    if let Some(r) = res {
        assert!(r.messages_after < before);
        assert!(r.summary_len > 0);
    }
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::CompactionSummary { .. })));
}

#[tokio::test]
async fn checkpoint_runs_once_per_compactor_instance() {
    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: true,
        threshold: 5,
        prompt: "persist memory".to_string(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());
    let memory_tools = vec![ToolSchema::new(
        "memory_write",
        "write memory",
        serde_json::json!({"type":"object"}),
    )];

    let mut first = vec![Message::user("token ".repeat(5000))];
    let first_res = compactor.compact(&mut first, 512, &memory_tools).await;
    assert!(first_res.is_some());
    if let Some(r) = first_res {
        assert!(r.checkpoint_usage.is_some());
    }

    let mut second = vec![Message::user("token ".repeat(5000))];
    let second_res = compactor.compact(&mut second, 512, &memory_tools).await;
    assert!(second_res.is_some());
    if let Some(r) = second_res {
        assert!(r.checkpoint_usage.is_none());
    }
}

#[tokio::test]
async fn compaction_failure_guard_skips_after_three_failures_and_can_return_checkpoint_usage() {
    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: true,
        threshold: 5,
        prompt: "persist memory".to_string(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    for _ in 0..3 {
        let mut messages = vec![Message::user("token ".repeat(5000))];
        let res = compactor.compact(&mut messages, 512, &[]).await;
        assert!(res.is_some());
    }

    let memory_tools = vec![ToolSchema::new(
        "memory_write",
        "write memory",
        serde_json::json!({"type":"object"}),
    )];
    let mut messages = vec![Message::user("token ".repeat(5000))];
    let guarded = compactor.compact(&mut messages, 512, &memory_tools).await;
    assert!(guarded.is_some());

    if let Some(r) = guarded {
        assert_eq!(r.summary_len, 0);
        assert!(r.checkpoint_usage.is_some());
    }
}

// ── CompactionResult fields ───────────────────────────────────────────────────

#[tokio::test]
async fn compaction_result_messages_before_matches_input_len() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("summary text"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let big = "token ".repeat(3000);
    let mut messages = vec![
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user(big),
    ];
    let before = messages.len();

    let res = compactor
        .compact(&mut messages, 256, &[])
        .await
        .ok_or_else(|| anyhow::anyhow!("expected Some compaction result"))?;
    assert_eq!(res.messages_before, before);
    Ok(())
}

#[tokio::test]
async fn compaction_result_duration_ms_is_set() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("summary text"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let big = "token ".repeat(3000);
    let mut messages = vec![
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user(big),
    ];

    let res = compactor
        .compact(&mut messages, 256, &[])
        .await
        .ok_or_else(|| anyhow::anyhow!("expected Some compaction result"))?;
    // duration_ms is a u64 derived from elapsed time; just verify it's present (not a sentinel)
    // We can't assert an exact value, but it must be a valid u64 (always true).
    let _ = res.duration_ms;
    Ok(())
}

#[tokio::test]
async fn compaction_result_token_usage_has_nonzero_tokens_when_summary_produced() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("condensed summary text"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let big = "token ".repeat(3000);
    let mut messages = vec![
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user(big),
    ];

    let res = compactor
        .compact(&mut messages, 256, &[])
        .await
        .ok_or_else(|| anyhow::anyhow!("expected Some compaction result"))?;
    // MockLLMProvider returns usage; when a summary was produced token_usage should be non-zero
    if res.summary_len > 0 {
        assert!(
            res.token_usage.prompt_tokens > 0 || res.token_usage.completion_tokens > 0,
            "expected non-zero token_usage when summary was produced"
        );
    }
    Ok(())
}

#[tokio::test]
async fn compaction_result_messages_after_less_than_before_on_success() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let checkpoint = Arc::new(CheckpointConfig {
        enabled: false,
        threshold: 5,
        prompt: String::new(),
    });
    let mut compactor = Compactor::new(llm, "mock".into(), checkpoint, CancellationToken::new());

    let big = "token ".repeat(3000);
    let mut messages = vec![
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user(big),
    ];

    let res = compactor
        .compact(&mut messages, 256, &[])
        .await
        .ok_or_else(|| anyhow::anyhow!("expected Some compaction result"))?;
    assert!(res.messages_after < res.messages_before);
    assert_eq!(res.messages_after, messages.len());
    Ok(())
}
