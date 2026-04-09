//! Context tracking, configuration, and execution limits.

use serde::Deserialize;
use serde::Serialize;

use super::tokens::message_tokens;
use super::tokens::total_tokens;
use crate::types::*;

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
