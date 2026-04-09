mod helpers;

use bendengine::context::*;
use bendengine::types::*;
use helpers::compact_helpers::*;
use proptest::prelude::*;

/// Strategy: generate a random message sequence from a pattern string.
/// Characters: u=user, t=tool_turn, a=assistant_text
/// Tool turns get unique ids and matched pairs.
fn build_messages_from_pattern(pattern: &str, pad: usize, tool_output: usize) -> Vec<AgentMessage> {
    let mut messages = Vec::new();
    let mut tool_idx = 0usize;
    for ch in pattern.chars() {
        match ch {
            'u' => messages.push(sized_user(pad)),
            'a' => messages.push(assistant_text("assistant response")),
            't' => {
                let id = format!("tc-prop-{tool_idx}");
                tool_idx += 1;
                messages.extend(tool_turn(&id, "bash", tool_output));
            }
            _ => {}
        }
    }
    messages
}

fn arb_pattern() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[uat]{1,25}")
        .expect("valid regex")
        .prop_filter("must have at least one user message", |s| s.contains('u'))
}

fn arb_config() -> impl Strategy<Value = ContextConfig> {
    (
        100..5000usize,
        0..200usize,
        1..15usize,
        0..5usize,
        10..50usize,
    )
        .prop_map(|(max, sys, recent, first, max_lines)| ContextConfig {
            max_context_tokens: max,
            system_prompt_tokens: sys,
            keep_recent: recent,
            keep_first: first,
            tool_output_max_lines: max_lines,
        })
}

fn arb_pad() -> impl Strategy<Value = usize> {
    prop_oneof![Just(10), Just(100), Just(500), Just(2000),]
}

fn arb_tool_output() -> impl Strategy<Value = usize> {
    prop_oneof![Just(10), Just(100), Just(1000), Just(5000),]
}

// ---------------------------------------------------------------------------
// P1: compact never produces orphan tool_call / tool_result
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_preserves_tool_pair_integrity(
        pattern in arb_pattern(),
        config in arb_config(),
        pad in arb_pad(),
        tool_output in arb_tool_output(),
    ) {
        let messages = build_messages_from_pattern(&pattern, pad, tool_output);
        if messages.is_empty() {
            return Ok(());
        }
        let result = compact_messages(messages, &config);
        assert_no_orphan_tool_pairs(&result.messages);
    }
}

// ---------------------------------------------------------------------------
// P2: level 0 is identity — no changes when within budget
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_level_zero_is_identity(
        pattern in arb_pattern(),
    ) {
        let messages = build_messages_from_pattern(&pattern, 10, 10);
        if messages.is_empty() {
            return Ok(());
        }
        let config = ContextConfig {
            max_context_tokens: 999_999,
            system_prompt_tokens: 0,
            keep_recent: 100,
            keep_first: 100,
            tool_output_max_lines: 100,
        };
        let original_len = messages.len();
        let result = compact_messages(messages, &config);
        prop_assert_eq!(result.stats.level, 0);
        prop_assert_eq!(result.messages.len(), original_len);
        prop_assert!(result.stats.actions.is_empty());
    }
}

// ---------------------------------------------------------------------------
// P3: actions method matches the reported level
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_actions_match_level(
        pattern in arb_pattern(),
        config in arb_config(),
        pad in arb_pad(),
        tool_output in arb_tool_output(),
    ) {
        let messages = build_messages_from_pattern(&pattern, pad, tool_output);
        if messages.is_empty() {
            return Ok(());
        }
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
// P4: action token accounting is self-consistent
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_action_tokens_are_consistent(
        pattern in arb_pattern(),
        config in arb_config(),
        pad in arb_pad(),
        tool_output in arb_tool_output(),
    ) {
        let messages = build_messages_from_pattern(&pattern, pad, tool_output);
        if messages.is_empty() {
            return Ok(());
        }
        let result = compact_messages(messages, &config);
        for action in &result.stats.actions {
            prop_assert!(
                action.after_tokens <= action.before_tokens,
                "action after_tokens ({}) should be <= before_tokens ({})",
                action.after_tokens,
                action.before_tokens,
            );
        }
    }
}
