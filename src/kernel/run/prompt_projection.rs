//! Prompt projection — decides which messages enter the LLM prompt
//! and provides the authoritative token counts for history budget assessment.

use crate::llm::tokens::count_tokens;
use crate::sessions::Message;

/// Whether a message will be included in the LLM prompt.
///
/// This is the single source of truth for prompt relevance.
/// Must stay in sync with `fmt::to_chat_messages()` filter logic.
pub fn is_prompt_relevant(m: &Message) -> bool {
    !matches!(
        m,
        Message::Memory { .. } | Message::Note { .. } | Message::OperationEvent { .. }
    )
}

/// Count tokens for messages that will actually be sent to the LLM.
///
/// This is the authoritative token count for history budget assessment.
/// Does **not** include `system_prompt` or `tools` — those live outside
/// the history budget controlled by `max_context_tokens`.
pub fn count_prompt_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| is_prompt_relevant(m))
        .map(|m| count_tokens(&m.text()))
        .sum()
}

/// Per-message prompt token counts; non-prompt messages get 0.
///
/// Used by compaction split planning so that non-prompt messages
/// don't consume budget in the split calculation.
pub fn prompt_token_vec(messages: &[Message]) -> Vec<usize> {
    messages
        .iter()
        .map(|m| {
            if is_prompt_relevant(m) {
                count_tokens(&m.text())
            } else {
                0
            }
        })
        .collect()
}
