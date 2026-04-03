use serde::Deserialize;
use serde::Serialize;

use crate::execution::checkpoint::CompactionCheckpoint;
use crate::llm::usage::TokenUsage;
use crate::sessions::Message;

/// A block of content in the agent's final response.
///
/// Supports mixed thinking + text output from extended-thinking models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// Visible text output.
    #[serde(rename = "text")]
    Text { text: String },
    /// Internal reasoning (extended thinking models).
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    pub fn thinking(s: impl Into<String>) -> Self {
        Self::Thinking { thinking: s.into() }
    }
}

/// Final result of an agent loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    /// The assistant's final response blocks (text + optional thinking).
    pub content: Vec<ContentBlock>,
    /// Number of LLM reasoning turns executed.
    pub iterations: u32,
    /// Accumulated token usage across all turns.
    pub usage: Usage,
    /// Why the loop stopped.
    pub stop_reason: Reason,
    /// Optional persistent checkpoint emitted by compaction during this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CompactionCheckpoint>,
    /// All messages produced during the session (system, user, assistant, tool_result, etc.).
    pub messages: Vec<Message>,
}

impl Result {
    /// Extract concatenated text content (ignoring thinking blocks).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Create an aborted result with no content.
    pub fn aborted() -> Self {
        Self {
            content: Vec::new(),
            iterations: 0,
            usage: Usage::default(),
            stop_reason: Reason::Aborted,
            checkpoint: None,
            messages: Vec::new(),
        }
    }
}

/// Why the agent loop stopped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Reason {
    /// The LLM finished naturally (no more tool calls).
    EndTurn,
    /// Hit the maximum iteration limit.
    MaxIterations,
    /// Hit the time deadline.
    Timeout,
    /// Cancelled via `CancellationToken`.
    Aborted,
    /// An unrecoverable error occurred.
    Error,
}

impl Reason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxIterations => "max_iterations",
            Self::Timeout => "timeout",
            Self::Aborted => "aborted",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Accumulated token usage across all turns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub ttft_ms: u64,
}

impl Usage {
    pub fn add(&mut self, usage: &TokenUsage) {
        self.prompt_tokens += usage.prompt_tokens as u64;
        self.completion_tokens += usage.completion_tokens as u64;
        self.total_tokens += usage.total_tokens as u64;
        self.cache_read_tokens += usage.cache_read_tokens as u64;
        self.cache_write_tokens += usage.cache_write_tokens as u64;
    }

    pub fn merge(&mut self, other: &Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.total_tokens += other.total_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        if self.ttft_ms == 0 && other.ttft_ms > 0 {
            self.ttft_ms = other.ttft_ms;
        }
    }

    /// Fraction of prompt tokens served from cache (0.0–1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        if self.prompt_tokens == 0 {
            return 0.0;
        }
        self.cache_read_tokens as f64 / self.prompt_tokens as f64
    }
}

/// Output of a completed run, suitable for CLI or API consumers.
/// Decoupled from persistence — assembled directly from AgentResult.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub text: String,
    pub stop_reason: Reason,
}
