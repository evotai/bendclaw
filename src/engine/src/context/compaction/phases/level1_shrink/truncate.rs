use std::collections::HashMap;

use crate::context::compaction::phases::level1_shrink::outline;
use crate::context::compaction::CompactionMethod;
use crate::types::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an index from tool_call_id → ToolCall arguments.
pub fn build_tool_call_index(messages: &[AgentMessage]) -> HashMap<String, serde_json::Value> {
    let mut index = HashMap::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for c in content {
                if let Content::ToolCall { id, arguments, .. } = c {
                    index.insert(id.clone(), arguments.clone());
                }
            }
        }
    }
    index
}

/// Byte limit for tool result text after line truncation.
/// Catches cases where individual lines are very long (minified JSON/HTML).
pub(super) const COMPACTION_MAX_BYTES: usize = 15_000;

/// Truncate content blocks using outline (if preferred) or head-tail,
/// then apply a byte cap to catch long single-line content.
pub fn truncate_content(
    content: &[Content],
    _tool_name: &str,
    tool_call_id: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    max_lines: usize,
    prefer_outline: bool,
) -> Vec<Content> {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => {
                let truncated = if prefer_outline {
                    try_outline_or_truncate(text, tool_call_index, tool_call_id, max_lines)
                } else {
                    truncate_text_head_tail(text, max_lines)
                };
                // Second pass: byte cap (line truncation alone may not be
                // enough when individual lines are very long).
                let truncated =
                    crate::tools::validation::truncate_tool_text(&truncated, COMPACTION_MAX_BYTES);
                Content::Text { text: truncated }
            }
            Content::Image { .. } => c.clone(),
            other => other.clone(),
        })
        .collect()
}

/// Try tree-sitter outline for read_file, fall back to head-tail.
fn try_outline_or_truncate(
    text: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    tool_call_id: &str,
    max_lines: usize,
) -> String {
    // Extract file path and extension from the tool call arguments
    if let Some(args) = tool_call_index.get(tool_call_id) {
        if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
            let ext = std::path::Path::new(path_str)
                .extension()
                .and_then(|e| e.to_str());
            if let Some(ext) = ext {
                if let Some(outlined) =
                    outline::extract_outline_from_read_file_output(text, ext, path_str)
                {
                    // Use outline only if it saves at least 10%
                    let threshold = text.len() / 10;
                    if outlined.len() + threshold < text.len() {
                        return outlined;
                    }
                }
            }
        }
    }

    truncate_text_head_tail(text, max_lines)
}

/// Truncate text keeping first N/2 and last N/2 lines.
pub fn truncate_text_head_tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }

    let head = max_lines / 2;
    let tail = max_lines - head;
    let omitted = lines.len() - head - tail;

    let mut result = lines[..head].join("\n");
    result.push_str(&format!("\n\n[... {} lines truncated ...]\n\n", omitted));
    result.push_str(&lines[lines.len() - tail..].join("\n"));
    result
}

/// Detect whether outline or head-tail was used by checking content.
pub fn detect_method(original: &[Content], truncated: &[Content]) -> CompactionMethod {
    for t in truncated {
        if let Content::Text { text } = t {
            if text.contains("[Structural outline of") {
                return CompactionMethod::Outline;
            }
            if text.contains("[... ") && text.contains(" lines truncated ...]") {
                return CompactionMethod::HeadTail;
            }
        }
    }
    // If content changed but no marker detected, it was still capped
    let orig_len: usize = original
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    let trunc_len: usize = truncated
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    if trunc_len < orig_len {
        CompactionMethod::OversizeCapped
    } else {
        CompactionMethod::HeadTail
    }
}
