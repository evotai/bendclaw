use crate::kernel::Message;

pub const POST_COMPACTION_TARGET: usize = 40_000;
pub const SUMMARY_RESERVE: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionPlan {
    pub split_index: usize,
    pub kept_tokens: usize,
}

pub fn keep_budget(max_context_tokens: usize) -> usize {
    POST_COMPACTION_TARGET
        .saturating_sub(SUMMARY_RESERVE)
        .min(max_context_tokens.saturating_sub(SUMMARY_RESERVE))
}

pub fn plan_compaction_split(
    messages: &[Message],
    msg_tokens: &[usize],
    max_context_tokens: usize,
) -> Option<CompactionPlan> {
    let keep_budget = keep_budget(max_context_tokens);
    let mut kept_tokens = 0;
    let mut split = messages.len();

    for i in (0..messages.len()).rev() {
        if matches!(
            messages[i],
            Message::System { .. } | Message::CompactionSummary { .. }
        ) {
            continue;
        }
        if kept_tokens + msg_tokens[i] > keep_budget {
            break;
        }
        kept_tokens += msg_tokens[i];
        split = i;
        if split > 0 && matches!(messages[split], Message::ToolResult { .. }) {
            split -= 1;
            kept_tokens += msg_tokens[split];
        }
    }

    if split == 0 {
        None
    } else {
        Some(CompactionPlan {
            split_index: split,
            kept_tokens,
        })
    }
}
