mod helpers;

use bendengine::context::*;
use helpers::message_pattern::*;
use proptest::prelude::*;

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
            prop_assert!(result.stats.actions.is_empty());
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
        if result.stats.level > 0 && result.stats.level <= 2 {
            let budget = config.max_context_tokens.saturating_sub(config.system_prompt_tokens);
            prop_assert!(
                result.stats.after_estimated_tokens <= budget,
                "level {} should respect budget: after={} > budget={}",
                result.stats.level,
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
