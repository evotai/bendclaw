//! Transcript building and text chunking — pure functions.

use crate::kernel::run::prompt_projection::is_prompt_relevant;
use crate::kernel::Message;

/// Build a transcript string from a slice of borrowed messages.
///
/// Only includes prompt-relevant messages so that L3 summaries
/// don't accidentally "bring back" Memory/Note/OperationEvent content.
pub fn build_transcript(dropped: &[&Message]) -> String {
    let mut transcript = String::new();
    for msg in dropped {
        if !is_prompt_relevant(msg) {
            continue;
        }
        let role = msg.role().map(|r| r.to_string()).unwrap_or("note".into());
        let text = msg.text();
        if !text.is_empty() {
            transcript.push_str(&format!("[{role}]: {text}\n\n"));
        }
    }
    transcript
}

/// Build a transcript string from a slice of owned messages.
///
/// Used by memory extractor before compaction.
pub fn build_transcript_from(messages: &[Message]) -> String {
    let refs: Vec<&Message> = messages.iter().collect();
    build_transcript(&refs)
}

/// Split text into chunks on paragraph boundaries.
pub fn split_chunks(text: &str, max_chars: usize) -> Vec<&str> {
    if text.len() <= max_chars {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chars).min(text.len());
        if end == text.len() {
            chunks.push(&text[start..]);
            break;
        }

        let slice = &text[start..end];
        let break_at = slice
            .rfind("\n\n")
            .map(|pos| start + pos + 2)
            .unwrap_or(end);

        chunks.push(&text[start..break_at]);
        start = break_at;
    }

    chunks
}
