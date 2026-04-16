use evotengine::context::*;
use evotengine::types::*;
use fixtures::compaction_assert::*;
use fixtures::message_dsl::*;
use proptest::prelude::*;

use super::fixtures;

/// Generate a random valid pattern from atomic units: "u", "a", "tr"
fn arb_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof!["u", "a", "tr"], 1..15)
        .prop_map(|v| v.concat())
        .prop_filter("must contain at least one u", |s| s.contains('u'))
}

/// Generate a random ContextConfig with ranges that cover all levels
fn arb_config() -> impl Strategy<Value = ContextConfig> {
    (
        100..5000usize,
        0..100usize,
        1..8usize,
        0..3usize,
        8..50usize,
    )
        .prop_map(|(max, sys, recent, first, max_lines)| ContextConfig {
            max_context_tokens: max,
            system_prompt_tokens: sys,
            keep_recent: recent,
            keep_first: first,
            tool_output_max_lines: max_lines,
        })
}

// ---------------------------------------------------------------------------
// P1: compact never produces orphan tool_call / tool_result
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_preserves_tool_pair_integrity(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        assert_no_orphan_tool_pairs(&result.messages);
    }
}

// ---------------------------------------------------------------------------
// P2: level 0 means messages are unchanged
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_level_zero_is_identity(
        pattern in arb_pattern(),
    ) {
        // Use a very large budget so compaction is never needed
        let messages = pat(&pattern).pad(10).tool_output(10).build();
        let config = ContextConfig {
            max_context_tokens: 500_000,
            system_prompt_tokens: 0,
            keep_recent: 100,
            keep_first: 100,
            tool_output_max_lines: 1000,
        };
        let original_len = messages.len();
        let result = compact_messages(messages, &config);
        prop_assert_eq!(result.stats.level, 0);
        prop_assert!(result.stats.actions.is_empty());
        prop_assert_eq!(result.messages.len(), original_len);
    }
}

// ---------------------------------------------------------------------------
// P3: actions method matches the reported level
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_actions_match_level(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        let level = result.stats.level;
        if level == 0 {
            for action in &result.stats.actions {
                prop_assert_eq!(action.method.clone(), CompactionMethod::LifecycleCleared);
            }
        } else {
            assert_actions_match_level(level, &result.stats.actions);
        }
    }
}

// ---------------------------------------------------------------------------
// P4: level <= 2 respects budget
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_respects_budget_before_level3(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        let budget = config.max_context_tokens.saturating_sub(config.system_prompt_tokens);

        // The pipeline always reduces or preserves token count
        prop_assert!(
            result.stats.after_estimated_tokens <= result.stats.before_estimated_tokens,
            "compaction should not increase tokens: before={} after={}",
            result.stats.before_estimated_tokens,
            result.stats.after_estimated_tokens,
        );

        // If we started under budget, we should stay under budget
        if result.stats.before_estimated_tokens <= budget {
            prop_assert!(
                result.stats.after_estimated_tokens <= budget,
                "under-budget input should stay under budget: after={} > budget={}",
                result.stats.after_estimated_tokens,
                budget,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// P5: action index range is valid
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_action_indices_are_valid(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let before_count = messages.len();
        let result = compact_messages(messages, &config);
        for action in &result.stats.actions {
            prop_assert!(
                action.index < before_count,
                "action index {} >= before_count {}",
                action.index, before_count,
            );
            if let Some(end) = action.end_index {
                prop_assert!(
                    end >= action.index,
                    "end_index {} < index {}",
                    end, action.index,
                );
                prop_assert!(
                    end < before_count,
                    "end_index {} >= before_count {}",
                    end, before_count,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// P6: each action's after_tokens <= before_tokens
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_action_tokens_monotonic(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        for action in &result.stats.actions {
            prop_assert!(
                action.after_tokens <= action.before_tokens,
                "action #{}: after {} > before {}",
                action.index, action.after_tokens, action.before_tokens,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// P7: sum of action savings <= overall savings
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_action_savings_bounded(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        let overall_saved = result.stats.before_estimated_tokens
            .saturating_sub(result.stats.after_estimated_tokens);
        let action_saved: usize = result.stats.actions.iter()
            .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
            .sum();
        prop_assert!(
            action_saved <= overall_saved,
            "action savings {} > overall savings {}",
            action_saved, overall_saved,
        );
    }
}

// ---------------------------------------------------------------------------
// Deterministic tests for keep_within_budget priority retention
// ---------------------------------------------------------------------------

#[test]
fn keep_within_budget_preserves_first_user_message() {
    // Build: user(small) → tool_call → tool_result(huge) → user(small)
    // With a tight budget, the first user message must survive.
    let messages = pat("u tr u").pad(10).tool_output(50_000).build();
    let config = ContextConfig {
        max_context_tokens: 200, // very tight
        system_prompt_tokens: 0,
        keep_recent: 100,
        keep_first: 2,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);

    // First message in result should be a user message containing "msg-0"
    let first = result
        .messages
        .first()
        .expect("should have at least one message");
    if let AgentMessage::Llm(Message::User { content, .. }) = first {
        let text = content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(
            text.contains("msg-0"),
            "first user message (task goal) should be preserved, got: {text}"
        );
    } else {
        panic!("first message should be a user message");
    }
}

#[test]
fn keep_within_budget_prefers_user_over_tool_result() {
    // Build: user → tool_call → tool_result(big) → user → tool_call → tool_result(big) → user
    // With tight budget, user messages should survive while tool results get dropped.
    let messages = pat("u tr u tr u").pad(10).tool_output(10_000).build();
    let config = ContextConfig {
        max_context_tokens: 300, // tight: enough for user msgs, not for tool results
        system_prompt_tokens: 0,
        keep_recent: 100,
        keep_first: 2,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);

    // Count surviving user messages vs tool results
    let user_count = result
        .messages
        .iter()
        .filter(|m| matches!(m, AgentMessage::Llm(Message::User { .. })))
        .count();
    let tool_result_count = result
        .messages
        .iter()
        .filter(|m| matches!(m, AgentMessage::Llm(Message::ToolResult { .. })))
        .count();

    // User messages should outnumber tool results when budget is tight
    assert!(
        user_count > tool_result_count,
        "user messages ({user_count}) should be prioritized over tool results ({tool_result_count})"
    );
}

/// Reproduce the exact scenario from the bug report: a single search tool
/// returns ~1.25M tokens, far exceeding the 156K budget.  Before the fix
/// the first user message (task goal) was dropped entirely.
#[test]
fn huge_tool_result_does_not_erase_task_goal() {
    // Simulate: user(small) → search_call → search_result(1.25M chars) → user(small)
    // tool_output of 1_250_000 chars ≈ the 1,251,126 tokens from the bug.
    let messages = pat("u tr u").pad(10).tool_output(1_250_000).build();
    let config = ContextConfig {
        max_context_tokens: 40_000, // ~156K / 4 (token estimate ratio)
        system_prompt_tokens: 1_000,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);

    // The first user message (task goal) must survive.
    let has_first_user = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content.iter().any(|c| match c {
                Content::Text { text } => text.contains("msg-0"),
                _ => false,
            })
        } else {
            false
        }
    });
    assert!(
        has_first_user,
        "first user message (task goal) must survive even with 1.25M token tool result"
    );

    // The last user message should also survive (it's recent + small).
    let has_last_user = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content.iter().any(|c| match c {
                Content::Text { text } => text.contains("msg-3"),
                _ => false,
            })
        } else {
            false
        }
    });
    assert!(
        has_last_user,
        "last user message should survive (small + recent)"
    );
}

/// When keep_first=3, the first 3 messages should all survive even under
/// extreme budget pressure, not just the first one.
#[test]
fn keep_first_preserves_multiple_leading_messages() {
    // Pattern: u u a tr tr u  (6 logical units = 8 messages)
    // keep_first=3 means msg-0(user), msg-1(user), msg-2(assistant) are protected.
    let messages = pat("u u a tr tr u").pad(10).tool_output(50_000).build();
    let config = ContextConfig {
        max_context_tokens: 300, // very tight — forces level 3 + keep_within_budget
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 3,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);

    // msg-0 (first user) must survive
    let has_msg0 = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content
                .iter()
                .any(|c| matches!(c, Content::Text { text } if text.contains("msg-0")))
        } else {
            false
        }
    });
    assert!(
        has_msg0,
        "msg-0 (first user) must survive with keep_first=3"
    );

    // msg-1 (second user) must survive
    let has_msg1 = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content
                .iter()
                .any(|c| matches!(c, Content::Text { text } if text.contains("msg-1")))
        } else {
            false
        }
    });
    assert!(
        has_msg1,
        "msg-1 (second user) must survive with keep_first=3"
    );

    // msg-2 (assistant text) must survive
    let has_msg2 = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = m {
            content
                .iter()
                .any(|c| matches!(c, Content::Text { text } if text.contains("msg-2")))
        } else {
            false
        }
    });
    assert!(has_msg2, "msg-2 (assistant) must survive with keep_first=3");
}

/// With keep_first=1 (default-like), only the very first message is protected.
/// The second user message may be dropped if budget is extremely tight.
#[test]
fn keep_first_one_only_protects_first_message() {
    // Pattern: u u tr u — 5 messages, huge tool output
    let messages = pat("u u tr u").pad(10).tool_output(100_000).build();
    let config = ContextConfig {
        max_context_tokens: 100, // absurdly tight
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);

    // msg-0 must survive (protected by keep_first=1)
    let has_msg0 = result.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content
                .iter()
                .any(|c| matches!(c, Content::Text { text } if text.contains("msg-0")))
        } else {
            false
        }
    });
    assert!(has_msg0, "msg-0 must survive with keep_first=1");
}
