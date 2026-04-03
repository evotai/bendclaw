use crate::llm::message::ChatMessage;
use crate::planning::prompt_projection::is_prompt_relevant;
use crate::sessions::Message;

/// Convert application-layer messages to LLM-compatible messages.
///
/// Pre-filters via `is_prompt_relevant()` (the single source of truth),
/// then converts each surviving message to a `ChatMessage`.
/// System prompts and compaction summaries are marked with `cache_control`
/// for Anthropic prompt caching (ignored by other providers).
pub fn to_chat_messages(messages: &[Message]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|m| is_prompt_relevant(m))
        .filter_map(|m| match m {
            Message::System { content } => Some(ChatMessage::system(content).with_cache_control()),

            Message::User { content, .. } => Some(ChatMessage::user_multimodal(content.clone())),

            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                if tool_calls.is_empty() {
                    Some(ChatMessage::assistant(content))
                } else {
                    Some(ChatMessage::assistant_with_tool_calls(
                        content,
                        tool_calls.clone(),
                    ))
                }
            }

            Message::ToolResult {
                tool_call_id,
                output,
                ..
            } => Some(ChatMessage::tool_result(tool_call_id, output)),

            Message::CompactionSummary { summary, .. } => Some(
                ChatMessage::system(format!("[Previous conversation summary]\n{summary}"))
                    .with_cache_control(),
            ),

            Message::Error { source, message } => {
                Some(ChatMessage::assistant(format!("[{source}] {message}")))
            }

            // is_prompt_relevant() already filtered these out, but the
            // compiler needs exhaustive matching.
            _ => None,
        })
        .collect()
}
