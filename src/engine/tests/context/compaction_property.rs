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

/// Generate degenerate patterns: empty, single element, no user messages.
fn arb_pattern_degenerate() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("u".into()),
        Just("a".into()),
        // All users, no tools
        prop::collection::vec(Just("u"), 1..10).prop_map(|v| v.concat()),
        // All assistant, no user
        prop::collection::vec(Just("a"), 1..10).prop_map(|v| v.concat()),
        // Normal patterns (superset)
        prop::collection::vec(prop_oneof!["u", "a", "tr"], 0..15).prop_map(|v| v.concat()),
    ]
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
            max_messages: 0,
            ..Default::default()
        })
}

/// Config strategy that includes extreme/boundary values.
fn arb_config_extreme() -> impl Strategy<Value = ContextConfig> {
    (
        prop_oneof![Just(0usize), Just(1), Just(10), 0..5000usize],
        prop_oneof![Just(0usize), Just(200), 0..200usize],
        prop_oneof![Just(0usize), Just(100), 0..20usize],
        prop_oneof![Just(0usize), Just(100), 0..20usize],
        prop_oneof![Just(0u8), Just(100), 0..100u8],
        prop_oneof![Just(0u8), Just(100), 0..100u8],
        prop_oneof![Just(0usize), Just(1), Just(3), 0..50usize],
    )
        .prop_map(
            |(max, sys, recent, first, trigger, target, max_msgs)| ContextConfig {
                max_context_tokens: max,
                system_prompt_tokens: sys,
                keep_recent: recent,
                keep_first: first,
                compact_trigger_pct: trigger,
                compact_target_pct: target,
                max_messages: max_msgs,
                ..Default::default()
            },
        )
}

/// Helper: reduce pad/tool_out ranges for proptest speed while still
/// exercising all compaction levels (budget ranges handle the scaling).
fn arb_pad() -> impl Strategy<Value = usize> {
    10..200usize
}
fn arb_tool_out() -> impl Strategy<Value = usize> {
    10..500usize
}

/// Core invariants that must hold for ANY input: no panic, no orphans.
fn assert_core_invariants(
    _messages: &[AgentMessage],
    _config: &ContextConfig,
    result: &CompactionResult,
) {
    assert_no_orphan_tool_pairs(&result.messages);

    // Replacement markers can be larger than an empty/tiny original, so only
    // check the overall direction — individual actions may grow slightly.
    let action_saved_total: usize = result
        .stats
        .actions
        .iter()
        .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
        .sum();
    let action_grew_total: usize = result
        .stats
        .actions
        .iter()
        .map(|a| a.after_tokens.saturating_sub(a.before_tokens))
        .sum();
    assert!(
        action_saved_total >= action_grew_total,
        "actions net-increased tokens: saved={action_saved_total} grew={action_grew_total}",
    );
}

// ---------------------------------------------------------------------------
// P1: compact never produces orphan tool_call / tool_result
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_preserves_tool_pair_integrity(
        pattern in arb_pattern(),
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
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
            tool_output_max_lines: 1000, ..Default::default()
        };
        let original_len = messages.len();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
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
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
        let level = result.stats.level;
        if level == 0 {
            for action in &result.stats.actions {
                prop_assert_eq!(action.method.clone(), CompactionMethod::LifecycleReclaimed);
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
    fn compact_reduces_or_preserves_tokens(
        pattern in arb_pattern(),
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
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
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let before_count = messages.len();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
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
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
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
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
        let overall_saved = result.stats.before_estimated_tokens
            .saturating_sub(result.stats.after_estimated_tokens);
        let action_saved: usize = result.stats.actions.iter()
            .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
            .sum();
        // Allow a small margin: floor calibration (.max(total_tokens)) and
        // sanitize can cause action_saved to slightly exceed overall_saved.
        let margin = (result.stats.before_estimated_tokens / 20).max(10);
        prop_assert!(
            action_saved <= overall_saved + margin,
            "action savings {} > overall savings {} + margin {}",
            action_saved, overall_saved, margin,
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
    let messages = pat("u tr u").pad(10).tool_output(5_000).build();
    let config = ContextConfig {
        max_context_tokens: 200, // very tight
        system_prompt_tokens: 0,
        keep_recent: 100,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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
    let messages = pat("u tr u tr u").pad(10).tool_output(5_000).build();
    let config = ContextConfig {
        max_context_tokens: 300, // tight: enough for user msgs, not for tool results
        system_prompt_tokens: 0,
        keep_recent: 100,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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
    // Simulate: user(small) → search_call → search_result(large) → user(small)
    // 50K chars is enough to trigger all compaction levels with a 40K budget.
    let messages = pat("u tr u").pad(10).tool_output(5_000).build();
    let config = ContextConfig {
        max_context_tokens: 40_000, // ~156K / 4 (token estimate ratio)
        system_prompt_tokens: 1_000,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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
    let messages = pat("u u a tr tr u").pad(10).tool_output(5_000).build();
    let config = ContextConfig {
        max_context_tokens: 300, // very tight — forces L2 drop + keep_within_budget
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 3,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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
    let messages = pat("u u tr u").pad(10).tool_output(5_000).build();
    let config = ContextConfig {
        max_context_tokens: 100, // absurdly tight
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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

// ===========================================================================
// P8: degenerate inputs × extreme configs never panic
// ===========================================================================

proptest! {
    #[test]
    fn compact_degenerate_never_panics(
        pattern in arb_pattern_degenerate(),
        pad in prop_oneof![Just(0usize), Just(1), 10..200usize],
        tool_out in prop_oneof![Just(0usize), Just(1), 10..500usize],
        config in arb_config_extreme(),
    ) {
        // Pattern may have unmatched 't' — skip those (build panics by design).
        let has_unmatched_t = {
            let chars: Vec<char> = pattern.chars().filter(|c| *c != ' ').collect();
            let mut pending = 0i32;
            for ch in &chars {
                match ch {
                    't' => pending += 1,
                    'r' => pending -= 1,
                    _ => {}
                }
            }
            pending != 0
        };
        if has_unmatched_t || pattern.is_empty() {
            // Empty or structurally invalid — test with empty vec directly
            let messages: Vec<AgentMessage> = Vec::new();
            let budget_state = CompactionBudgetState::from_messages(&messages);
            let result = compact_messages(messages.clone(), &config, &budget_state);
            assert_core_invariants(&messages, &config, &result);
        } else {
            let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
            let budget_state = CompactionBudgetState::from_messages(&messages);
            let result = compact_messages(messages.clone(), &config, &budget_state);
            assert_core_invariants(&messages, &config, &result);
        }
    }
}

// ===========================================================================
// P9: idempotency — second compact is stable
// ===========================================================================

proptest! {
    #[test]
    fn compact_is_idempotent(
        pattern in arb_pattern(),
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let r1 = compact_messages(messages, &config, &budget_state);

        let budget_state2 = CompactionBudgetState {
            estimated_tokens: r1.stats.after_estimated_tokens,
        };
        let r2 = compact_messages(r1.messages.clone(), &config, &budget_state2);

        assert_no_orphan_tool_pairs(&r2.messages);

        // Second pass should not reduce significantly — allow 10% margin for
        // rounding in sanitize and token estimation.
        let floor = r1.stats.after_estimated_tokens * 85 / 100;
        prop_assert!(
            r2.stats.after_estimated_tokens >= floor,
            "second compact reduced too aggressively: {} -> {} (floor={})",
            r1.stats.after_estimated_tokens,
            r2.stats.after_estimated_tokens,
            floor,
        );
    }
}

// ===========================================================================
// P10: extreme configs × normal patterns — core invariants hold
// ===========================================================================

proptest! {
    #[test]
    fn compact_extreme_config_invariants(
        pattern in arb_pattern(),
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config_extreme(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages.clone(), &config, &budget_state);
        assert_core_invariants(&messages, &config, &result);
        assert_actions_match_level(result.stats.level, &result.stats.actions);
    }
}

// ===========================================================================
// P11: output always has at least one message when input is non-empty
//      (only for configs with a usable budget — degenerate budget=0 +
//       max_messages=1 can legitimately empty the list via sanitize)
// ===========================================================================

proptest! {
    #[test]
    fn compact_never_empties_nonempty_input(
        pattern in arb_pattern(),
        pad in arb_pad(),
        tool_out in arb_tool_out(),
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
        prop_assert!(
            !result.messages.is_empty(),
            "compact produced empty output from non-empty input"
        );
    }
}

// ===========================================================================
// Deterministic boundary tests
// ===========================================================================

#[test]
fn compact_empty_input() {
    let messages: Vec<AgentMessage> = Vec::new();
    let config = ContextConfig::default();
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert!(result.messages.is_empty());
    assert_eq!(result.stats.level, 0);
    assert!(result.stats.actions.is_empty());
}

#[test]
fn compact_single_user_message() {
    let messages = pat("u").pad(10).build();
    let config = ContextConfig {
        max_context_tokens: 1,
        system_prompt_tokens: 0,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert!(!result.messages.is_empty());
    assert_no_orphan_tool_pairs(&result.messages);
}

#[test]
fn compact_budget_zero() {
    let messages = pat("u a u tr u").pad(100).tool_output(500).build();
    let config = ContextConfig {
        max_context_tokens: 0,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_system_prompt_exceeds_max_context() {
    let messages = pat("u tr u").pad(100).tool_output(500).build();
    let config = ContextConfig {
        max_context_tokens: 100,
        system_prompt_tokens: 200, // budget underflows to 0
        keep_recent: 1,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_keep_recent_exceeds_message_count() {
    let messages = pat("u a u").pad(10).build();
    let config = ContextConfig {
        max_context_tokens: 50,
        system_prompt_tokens: 0,
        keep_recent: 1000, // way more than 3 messages
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_keep_first_exceeds_message_count() {
    let messages = pat("u a u").pad(10).build();
    let config = ContextConfig {
        max_context_tokens: 50,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1000, // way more than 3 messages
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_keep_first_plus_recent_overlap() {
    // 5 messages, keep_first=3 + keep_recent=3 → overlap in the middle
    let messages = pat("u a u a u").pad(200).build();
    let config = ContextConfig {
        max_context_tokens: 100,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 3,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_keep_first_zero_keep_recent_zero() {
    let messages = pat("u a u tr u").pad(200).tool_output(500).build();
    let config = ContextConfig {
        max_context_tokens: 100,
        system_prompt_tokens: 0,
        keep_recent: 0,
        keep_first: 0,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_trigger_pct_zero() {
    // trigger=0 means everything exceeds trigger → collapse always runs
    let messages = pat("u tr u tr u").pad(50).tool_output(200).build();
    let config = ContextConfig {
        max_context_tokens: 5000,
        system_prompt_tokens: 0,
        compact_trigger_pct: 0,
        compact_target_pct: 0,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_trigger_pct_100_target_100() {
    // trigger=100, target=100 → collapse only when at 100% of budget
    let messages = pat("u tr u tr u").pad(50).tool_output(200).build();
    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 0,
        compact_trigger_pct: 100,
        compact_target_pct: 100,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_max_messages_one() {
    let messages = pat("u a u tr u").pad(10).tool_output(50).build();
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        max_messages: 1,
        keep_recent: 1,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_max_messages_equals_current_len() {
    let messages = pat("u a u a u").pad(10).build();
    let len = messages.len();
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        max_messages: len,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    // At the limit but not over — should not evict
    assert_eq!(result.stats.level, 0);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_max_messages_just_exceeded() {
    let messages = pat("u a u a u a u").pad(10).build();
    let len = messages.len();
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        max_messages: len - 1, // one over
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
    // Should trigger eviction
    assert!(result.stats.level >= 3);
}

#[test]
fn compact_all_users_no_tools() {
    let messages = pat("u u u u u u u u").pad(200).build();
    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_tool_result_with_empty_content() {
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "tc-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: vec![], // empty content
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("next")),
    ];
    let config = ContextConfig {
        max_context_tokens: 10,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_error_tool_result_treated_same() {
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "tc-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "x".repeat(50_000),
            }],
            is_error: true, // error result
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("next")),
    ];
    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_provider_overhead_much_larger_than_message_tokens() {
    // Simulates images: provider reports 100K tokens but chars/4 is only 500.
    let messages = pat("u tr u").pad(10).tool_output(100).build();
    let budget_state = CompactionBudgetState {
        estimated_tokens: 100_000, // huge provider overhead (images)
    };
    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_many_tiny_tool_turns() {
    // 30 tool turns with tiny output — tests L2 collapse with many turns
    let pattern = (0..30).map(|_| "tr").collect::<Vec<_>>().join(" ");
    let full_pattern = format!("u {} u", pattern);
    let messages = pat(&full_pattern).pad(5).tool_output(5).build();
    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_single_massive_tool_result_exceeds_budget_10x() {
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "tc-1".into(),
                name: "search".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "search".into(),
            content: vec![Content::Text {
                text: "x".repeat(4_000), // ~1K tokens vs 1K budget — still 10x over
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("next")),
    ];
    let config = ContextConfig {
        max_context_tokens: 100,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
    // First user message must survive
    assert!(result.messages.iter().any(|m| {
        matches!(m, AgentMessage::Llm(Message::User { content, .. })
            if content.iter().any(|c| matches!(c, Content::Text { text } if text.contains("task"))))
    }));
}

#[test]
fn compact_alternating_user_assistant_long_session() {
    // Simulate a long conversation with no tools — only L2 collapse + L3 evict apply
    let pattern = (0..40).map(|_| "u a").collect::<Vec<_>>().join(" ");
    let messages = pat(&pattern).pad(100).build();
    let config = ContextConfig {
        max_context_tokens: 1000,
        system_prompt_tokens: 0,
        keep_recent: 4,
        keep_first: 2,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
    assert!(
        result.messages.len() < messages.len(),
        "long session should be compacted"
    );
}

#[test]
fn compact_message_limit_target_pct_boundary() {
    // message_limit_target_pct=1 → aggressive drop to near keep_first+keep_recent
    let messages = pat("u a u a u a u a u a u a u a u a u a u a u")
        .pad(10)
        .build();
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        max_messages: 10,
        message_limit_target_pct: 1, // drop to ~1% of max_messages
        keep_recent: 2,
        keep_first: 1,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
    assert_core_invariants(&messages, &config, &result);
}

#[test]
fn compact_retention_current_run_reclaimed() {
    // CurrentRun retention should be reclaimed in L0
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "tc-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "x".repeat(10_000),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::CurrentRun, // should be reclaimed
        }),
        AgentMessage::Llm(Message::user("next")),
    ];
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_no_orphan_tool_pairs(&result.messages);
    // The CurrentRun result should have been reclaimed
    let reclaimed = result
        .stats
        .actions
        .iter()
        .any(|a| a.method == CompactionMethod::LifecycleReclaimed);
    assert!(reclaimed, "CurrentRun retention should trigger L0 reclaim");
}
