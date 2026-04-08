//! Context window management — smart truncation and token counting.
//!
//! The #1 engineering challenge for agents. This module provides:
//! - Token estimation (fast, no external deps)
//! - Tiered compaction (tool output truncation → turn summarization → full summary)
//! - Execution limits (max turns, tokens, duration)
//!
//! Designed based on Claude Code's approach: clear old tool outputs first,
//! then summarize conversation if needed.

use serde::Deserialize;
use serde::Serialize;

use crate::types::*;

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Rough token estimate: ~4 chars per token for English text.
/// Good enough for context budgeting. Use tiktoken-rs for precision.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Estimate tokens for a single message
pub fn message_tokens(msg: &AgentMessage) -> usize {
    match msg {
        AgentMessage::Llm(m) => match m {
            Message::User { content, .. } => content_tokens(content) + 4,
            Message::Assistant { content, .. } => content_tokens(content) + 4,
            Message::ToolResult {
                content, tool_name, ..
            } => content_tokens(content) + estimate_tokens(tool_name) + 8,
        },
        AgentMessage::Extension(ext) => estimate_tokens(&ext.data.to_string()) + 4,
    }
}

pub fn content_tokens(content: &[Content]) -> usize {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => estimate_tokens(text),
            Content::Image { data, .. } => {
                // Estimate tokens from base64 data length:
                // base64 len * 3/4 = raw bytes; ~750 bytes per token for images.
                // Floor at 85 (Anthropic minimum), cap at 16000.
                let raw_bytes = data.len() * 3 / 4;
                (raw_bytes / 750).clamp(85, 16_000)
            }
            Content::Thinking { thinking, .. } => estimate_tokens(thinking),
            Content::ToolCall {
                name, arguments, ..
            } => estimate_tokens(name) + estimate_tokens(&arguments.to_string()) + 8,
        })
        .sum()
}

/// Estimate total tokens for a message list
pub fn total_tokens(messages: &[AgentMessage]) -> usize {
    messages.iter().map(message_tokens).sum()
}

// ---------------------------------------------------------------------------
// Context tracking (real usage + estimates)
// ---------------------------------------------------------------------------

/// Tracks context size using real token counts from provider responses
/// combined with estimates for messages added after the last response.
///
/// This gives more accurate context size tracking than pure estimation,
/// since providers report actual token counts in their usage data.
///
/// # Example
///
/// ```rust
/// use bendengine::context::ContextTracker;
/// use bendengine::types::Usage;
///
/// let mut tracker = ContextTracker::new();
/// // After receiving an assistant response with usage data:
/// tracker.record_usage(
///     &Usage {
///         input: 1500,
///         output: 200,
///         ..Default::default()
///     },
///     3,
/// );
/// ```
pub struct ContextTracker {
    /// Last known total token count from provider usage
    last_usage_tokens: Option<usize>,
    /// Index of the message that had the last usage
    last_usage_index: Option<usize>,
}

impl ContextTracker {
    pub fn new() -> Self {
        Self {
            last_usage_tokens: None,
            last_usage_index: None,
        }
    }

    /// Record usage from an assistant response.
    ///
    /// Call this after each assistant message to update the tracker
    /// with real token counts from the provider.
    pub fn record_usage(&mut self, usage: &Usage, message_index: usize) {
        let total = usage.input + usage.output + usage.cache_read + usage.cache_write;
        if total > 0 {
            self.last_usage_tokens = Some(total as usize);
            self.last_usage_index = Some(message_index);
        }
    }

    /// Estimate current context size.
    ///
    /// Uses real usage from the last assistant response as a baseline,
    /// then adds estimates (chars/4) for any messages added since.
    /// Falls back to pure estimation if no usage data is available.
    pub fn estimate_context_tokens(&self, messages: &[AgentMessage]) -> usize {
        match (self.last_usage_tokens, self.last_usage_index) {
            (Some(usage_tokens), Some(idx)) if idx < messages.len() => {
                let trailing: usize = messages[idx + 1..].iter().map(message_tokens).sum();
                usage_tokens + trailing
            }
            _ => total_tokens(messages),
        }
    }

    /// Reset tracking (e.g. after compaction replaces messages).
    pub fn reset(&mut self) {
        self.last_usage_tokens = None;
        self.last_usage_index = None;
    }
}

impl Default for ContextTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Context configuration
// ---------------------------------------------------------------------------

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum context tokens (leave room for response)
    pub max_context_tokens: usize,
    /// Tokens reserved for the system prompt
    pub system_prompt_tokens: usize,
    /// Minimum recent messages to always keep (full detail)
    pub keep_recent: usize,
    /// Minimum first messages to always keep
    pub keep_first: usize,
    /// Max lines to keep per tool output in Level 1 compaction
    pub tool_output_max_lines: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            system_prompt_tokens: 4_000,
            keep_recent: 10,
            keep_first: 2,
            tool_output_max_lines: 50,
        }
    }
}

impl ContextConfig {
    /// Derive a context config from a model's context window size.
    ///
    /// Reserves 20% of the context window for output tokens, uses the rest
    /// as the compaction budget. All other settings use defaults.
    pub fn from_context_window(context_window: u32) -> Self {
        let max_context_tokens = (context_window as usize) * 80 / 100;
        Self {
            max_context_tokens,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Compaction strategy
// ---------------------------------------------------------------------------

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
}

/// Collect per-tool token details from messages, sorted by tokens descending.
fn collect_tool_details(messages: &[AgentMessage]) -> Vec<ToolTokenDetail> {
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

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<AgentMessage>,
    pub stats: CompactionStats,
}

pub trait CompactionStrategy: Send + Sync {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult;
}

pub struct DefaultCompaction;

impl CompactionStrategy for DefaultCompaction {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
        compact_messages(messages, config)
    }
}

// ---------------------------------------------------------------------------
// Tiered compaction
// ---------------------------------------------------------------------------

/// Compact messages to fit within the token budget using tiered strategy.
///
/// - Level 1: Truncate tool outputs (keep head + tail)
/// - Level 2: Summarize old turns (replace details with one-liner)
/// - Level 3: Drop old messages (keep first + recent only)
///
/// Each level is tried in order. Returns as soon as messages fit.
pub fn compact_messages(messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);

    let before_message_count = messages.len();
    let before_estimated_tokens = total_tokens(&messages);
    let before_tool_details = collect_tool_details(&messages);

    let make_result = |msgs: Vec<AgentMessage>, level: u8, stats: CompactionStats| {
        let after_message_count = msgs.len();
        let after_estimated_tokens = total_tokens(&msgs);
        let after_tool_details = collect_tool_details(&msgs);
        CompactionResult {
            messages: msgs,
            stats: CompactionStats {
                level,
                before_message_count,
                after_message_count,
                before_estimated_tokens,
                after_estimated_tokens,
                before_tool_details: before_tool_details.clone(),
                after_tool_details,
                ..stats
            },
        }
    };

    if before_estimated_tokens <= budget {
        return make_result(messages, 0, CompactionStats::default());
    }

    let (compacted, tool_outputs_truncated) =
        level1_truncate_tool_outputs(&messages, config.tool_output_max_lines);
    if total_tokens(&compacted) <= budget {
        return make_result(compacted, 1, CompactionStats {
            tool_outputs_truncated,
            ..Default::default()
        });
    }

    let (compacted, turns_summarized) = level2_summarize_old_turns(&compacted, config.keep_recent);
    if total_tokens(&compacted) <= budget {
        return make_result(compacted, 2, CompactionStats {
            tool_outputs_truncated,
            turns_summarized,
            ..Default::default()
        });
    }

    let (compacted, messages_dropped) = level3_drop_middle(&compacted, config, budget);
    make_result(compacted, 3, CompactionStats {
        tool_outputs_truncated,
        turns_summarized,
        messages_dropped,
        ..Default::default()
    })
}

/// Level 1: Truncate long tool outputs to head + tail.
///
/// This is the cheapest compaction — preserves conversation structure,
/// just removes verbose tool output middles. In practice this saves
/// 50-70% of context in coding sessions.
fn level1_truncate_tool_outputs(
    messages: &[AgentMessage],
    max_lines: usize,
) -> (Vec<AgentMessage>, usize) {
    let mut truncated_count = 0;
    let result = messages
        .iter()
        .map(|msg| match msg {
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
            }) => {
                let mut was_truncated = false;
                let truncated_content: Vec<Content> = content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => {
                            let result = truncate_text_head_tail(text, max_lines);
                            if result.len() < text.len() {
                                was_truncated = true;
                            }
                            Content::Text { text: result }
                        }
                        other => other.clone(),
                    })
                    .collect();
                if was_truncated {
                    truncated_count += 1;
                }
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: truncated_content,
                    is_error: *is_error,
                    timestamp: *timestamp,
                })
            }
            other => other.clone(),
        })
        .collect();
    (result, truncated_count)
}

/// Truncate text keeping first N/2 and last N/2 lines.
fn truncate_text_head_tail(text: &str, max_lines: usize) -> String {
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

/// Level 2: Summarize old assistant turns.
///
/// Keeps the last `keep_recent` messages in full detail.
/// For older messages: assistant messages with tool calls get replaced
/// with a short summary, and their tool results get dropped.
fn level2_summarize_old_turns(
    messages: &[AgentMessage],
    keep_recent: usize,
) -> (Vec<AgentMessage>, usize) {
    let len = messages.len();
    if len <= keep_recent {
        return (messages.to_vec(), 0);
    }

    let boundary = len - keep_recent;
    let mut result = Vec::new();
    let mut turns_summarized = 0;

    let mut i = 0;
    while i < boundary {
        let msg = &messages[i];
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let text_parts: Vec<&str> = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::Text { text } => {
                            if text.len() > 200 {
                                None
                            } else {
                                Some(text.as_str())
                            }
                        }
                        _ => None,
                    })
                    .collect();

                let tool_count = content
                    .iter()
                    .filter(|c| matches!(c, Content::ToolCall { .. }))
                    .count();

                let summary = if !text_parts.is_empty() {
                    text_parts.join(" ")
                } else if tool_count > 0 {
                    format!("[Assistant used {} tool(s)]", tool_count)
                } else {
                    "[Assistant response]".into()
                };

                result.push(AgentMessage::Llm(Message::User {
                    content: vec![Content::Text {
                        text: format!("[Summary] {}", summary),
                    }],
                    timestamp: now_ms(),
                }));
                turns_summarized += 1;

                i += 1;
                while i < boundary {
                    if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[i] {
                        i += 1;
                    } else {
                        break;
                    }
                }
                continue;
            }
            AgentMessage::Llm(Message::ToolResult { .. }) => {
                i += 1;
                continue;
            }
            other => {
                result.push(other.clone());
            }
        }
        i += 1;
    }

    result.extend_from_slice(&messages[boundary..]);
    (result, turns_summarized)
}

/// Level 3: Drop middle messages, keeping first + recent.
fn level3_drop_middle(
    messages: &[AgentMessage],
    config: &ContextConfig,
    budget: usize,
) -> (Vec<AgentMessage>, usize) {
    let len = messages.len();
    let first_end = config.keep_first.min(len);
    let recent_start = len.saturating_sub(config.keep_recent);

    if first_end >= recent_start {
        let result = keep_within_budget(messages, budget);
        let dropped = len.saturating_sub(result.len());
        return (result, dropped);
    }

    let first_msgs = &messages[..first_end];
    let recent_msgs = &messages[recent_start..];
    let removed = recent_start - first_end;

    let marker = AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: format!(
                "[Context compacted: {} messages removed to fit context window]",
                removed
            ),
        }],
        timestamp: now_ms(),
    });

    let mut result = first_msgs.to_vec();
    result.push(marker);
    result.extend_from_slice(recent_msgs);

    if total_tokens(&result) > budget {
        let result = keep_within_budget(&result, budget);
        let dropped = len.saturating_sub(result.len());
        return (result, dropped);
    }

    (result, removed)
}

/// Keep as many recent messages as fit within budget.
fn keep_within_budget(messages: &[AgentMessage], budget: usize) -> Vec<AgentMessage> {
    let mut result = Vec::new();
    let mut remaining = budget;

    for msg in messages.iter().rev() {
        let tokens = message_tokens(msg);
        if tokens > remaining {
            break;
        }
        remaining -= tokens;
        result.push(msg.clone());
    }

    result.reverse();

    if result.len() < messages.len() {
        let removed = messages.len() - result.len();
        result.insert(
            0,
            AgentMessage::Llm(Message::User {
                content: vec![Content::Text {
                    text: format!("[Context compacted: {} messages removed]", removed),
                }],
                timestamp: now_ms(),
            }),
        );
    }

    result
}

// ---------------------------------------------------------------------------
// Execution limits
// ---------------------------------------------------------------------------

/// Execution limits for the agent loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    /// Maximum number of turns (LLM calls)
    pub max_turns: usize,
    /// Maximum total tokens consumed
    pub max_total_tokens: usize,
    /// Maximum wall-clock time
    pub max_duration: std::time::Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_total_tokens: 1_000_000,
            max_duration: std::time::Duration::from_secs(600),
        }
    }
}

/// Tracks execution state against limits
pub struct ExecutionTracker {
    pub limits: ExecutionLimits,
    pub turns: usize,
    pub tokens_used: usize,
    pub started_at: std::time::Instant,
}

impl ExecutionTracker {
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            turns: 0,
            tokens_used: 0,
            started_at: std::time::Instant::now(),
        }
    }

    pub fn record_turn(&mut self, tokens: usize) {
        self.turns += 1;
        self.tokens_used += tokens;
    }

    /// Check if any limit has been exceeded. Returns the reason if so.
    pub fn check_limits(&self) -> Option<String> {
        if self.turns >= self.limits.max_turns {
            return Some(format!(
                "Max turns reached ({}/{})",
                self.turns, self.limits.max_turns
            ));
        }
        if self.tokens_used >= self.limits.max_total_tokens {
            return Some(format!(
                "Max tokens reached ({}/{})",
                self.tokens_used, self.limits.max_total_tokens
            ));
        }
        let elapsed = self.started_at.elapsed();
        if elapsed >= self.limits.max_duration {
            return Some(format!(
                "Max duration reached ({:.0}s/{:.0}s)",
                elapsed.as_secs_f64(),
                self.limits.max_duration.as_secs_f64()
            ));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert!(estimate_tokens("hello world") > 0);
        assert!(estimate_tokens("hello world") < 10);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_context_config_from_context_window() {
        let config = ContextConfig::from_context_window(200_000);
        assert_eq!(config.max_context_tokens, 160_000); // 80% of 200K
        assert_eq!(config.system_prompt_tokens, 4_000); // default
        assert_eq!(config.keep_recent, 10); // default

        let config = ContextConfig::from_context_window(1_000_000);
        assert_eq!(config.max_context_tokens, 800_000); // 80% of 1M

        let config = ContextConfig::from_context_window(128_000);
        assert_eq!(config.max_context_tokens, 102_400); // 80% of 128K
    }

    #[test]
    fn test_truncate_head_tail() {
        let text = (1..=100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_text_head_tail(&text, 10);
        assert!(result.contains("line 1"));
        assert!(result.contains("line 5")); // head
        assert!(result.contains("line 100")); // tail
        assert!(result.contains("truncated"));
        assert!(!result.contains("line 50")); // middle removed
    }

    #[test]
    fn test_level1_truncation() {
        let big_output = (1..=200)
            .map(|i| format!("output line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let messages = vec![
            AgentMessage::Llm(Message::user("do something")),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                content: vec![Content::Text { text: big_output }],
                is_error: false,
                timestamp: 0,
            }),
        ];

        let compacted = level1_truncate_tool_outputs(&messages, 20);
        let tool_msg = &compacted.0[1];
        if let AgentMessage::Llm(Message::ToolResult { content, .. }) = tool_msg {
            if let Content::Text { text } = &content[0] {
                assert!(text.contains("truncated"));
                assert!(text.contains("output line 1")); // head
                assert!(text.contains("output line 200")); // tail
                assert!(text.lines().count() < 50);
            } else {
                panic!("expected text content");
            }
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_compact_within_budget() {
        let messages = vec![
            AgentMessage::Llm(Message::user("Hello")),
            AgentMessage::Llm(Message::user("World")),
        ];
        let config = ContextConfig::default();
        let result = compact_messages(messages.clone(), &config);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn test_compact_drops_middle_when_needed() {
        let mut messages = Vec::new();
        for i in 0..100 {
            messages.push(AgentMessage::Llm(Message::user(format!(
                "Message {} {}",
                i,
                "x".repeat(200)
            ))));
        }

        let config = ContextConfig {
            max_context_tokens: 500,
            system_prompt_tokens: 100,
            keep_recent: 5,
            keep_first: 2,
            tool_output_max_lines: 20,
        };

        let result = compact_messages(messages, &config);
        assert!(result.messages.len() < 100);
        assert!(result.messages.len() >= 2);
    }

    #[test]
    fn test_context_tracker_no_usage() {
        let tracker = ContextTracker::new();
        let messages = vec![
            AgentMessage::Llm(Message::user("Hello")),
            AgentMessage::Llm(Message::user("World")),
        ];
        // Without usage data, falls back to estimation
        let tokens = tracker.estimate_context_tokens(&messages);
        assert!(tokens > 0);
        assert_eq!(tokens, total_tokens(&messages));
    }

    #[test]
    fn test_context_tracker_with_usage() {
        let mut tracker = ContextTracker::new();
        let messages = vec![
            AgentMessage::Llm(Message::user("Hello")),
            AgentMessage::Llm(Message::Assistant {
                content: vec![Content::Text {
                    text: "Hi there!".into(),
                }],
                stop_reason: StopReason::Stop,
                model: "test".into(),
                provider: "test".into(),
                usage: Usage {
                    input: 100,
                    output: 50,
                    ..Default::default()
                },
                timestamp: 0,
                error_message: None,
            }),
            AgentMessage::Llm(Message::user("Follow up question here")),
        ];
        // Record usage at index 1 (assistant message)
        tracker.record_usage(
            &Usage {
                input: 100,
                output: 50,
                ..Default::default()
            },
            1,
        );
        let tokens = tracker.estimate_context_tokens(&messages);
        // Should be 150 (real usage) + estimate for the trailing user message
        let trailing_estimate = message_tokens(&messages[2]);
        assert_eq!(tokens, 150 + trailing_estimate);
    }

    #[test]
    fn test_context_tracker_reset() {
        let mut tracker = ContextTracker::new();
        tracker.record_usage(
            &Usage {
                input: 1000,
                output: 500,
                ..Default::default()
            },
            5,
        );
        tracker.reset();
        let messages = vec![AgentMessage::Llm(Message::user("test"))];
        // After reset, should fall back to estimation
        assert_eq!(
            tracker.estimate_context_tokens(&messages),
            total_tokens(&messages)
        );
    }

    #[test]
    fn test_execution_limits() {
        let limits = ExecutionLimits {
            max_turns: 3,
            max_total_tokens: 1000,
            max_duration: std::time::Duration::from_secs(60),
        };

        let mut tracker = ExecutionTracker::new(limits);
        assert!(tracker.check_limits().is_none());

        tracker.record_turn(100);
        tracker.record_turn(100);
        assert!(tracker.check_limits().is_none());

        tracker.record_turn(100);
        assert!(tracker.check_limits().is_some());
    }
}
