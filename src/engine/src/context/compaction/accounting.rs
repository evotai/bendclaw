use super::types::CompactionAction;
use super::types::CompactionMethod;
use super::types::CompactionStats;
use super::types::ToolTokenDetail;
use crate::context::tokens::content_tokens;
use crate::types::*;

/// Collect per-tool token details from messages, sorted by tokens descending.
pub fn collect_tool_details(messages: &[AgentMessage]) -> Vec<ToolTokenDetail> {
    let mut details = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            details.push(ToolTokenDetail {
                tool_name: tool_name.clone(),
                tokens: content_tokens(content),
            });
        }
    }
    details.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    details
}

pub fn image_count(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .map(|msg| match msg {
            AgentMessage::Llm(Message::User { content, .. })
            | AgentMessage::Llm(Message::Assistant { content, .. })
            | AgentMessage::Llm(Message::ToolResult { content, .. }) => content
                .iter()
                .filter(|c| matches!(c, Content::Image { .. }))
                .count(),
            AgentMessage::Extension(_) => 0,
        })
        .sum()
}

pub struct StatsInput {
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub before_tool_details: Vec<ToolTokenDetail>,
    pub after_tool_details: Vec<ToolTokenDetail>,
    pub actions: Vec<CompactionAction>,
}

pub fn build_stats(input: StatsInput) -> CompactionStats {
    let mut current_run_cleared: usize = 0;
    let mut age_cleared: usize = 0;
    let mut oversize_capped: usize = 0;
    let mut tool_outputs_truncated: usize = 0;
    let mut turns_summarized: usize = 0;
    let mut messages_dropped: usize = 0;

    for action in &input.actions {
        match action.method {
            CompactionMethod::LifecycleReclaimed => current_run_cleared += 1,
            CompactionMethod::AgeCleared => age_cleared += 1,
            CompactionMethod::OversizeCapped => oversize_capped += 1,
            CompactionMethod::Outline
            | CompactionMethod::HeadTail
            | CompactionMethod::ImageStripped => tool_outputs_truncated += 1,
            CompactionMethod::TurnCollapsed => turns_summarized += 1,
            CompactionMethod::MessagesEvicted => {
                messages_dropped += action.related_count.unwrap_or(1);
            }
        }
    }

    CompactionStats {
        level: input.level,
        before_message_count: input.before_message_count,
        after_message_count: input.after_message_count,
        before_estimated_tokens: input.before_estimated_tokens,
        after_estimated_tokens: input.after_estimated_tokens,
        tool_outputs_truncated,
        turns_summarized,
        messages_dropped,
        current_run_cleared,
        oversize_capped,
        age_cleared,
        before_tool_details: input.before_tool_details,
        after_tool_details: input.after_tool_details,
        actions: input.actions,
    }
}
