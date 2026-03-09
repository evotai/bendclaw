use std::ops::AddAssign;

use serde::Deserialize;
use serde::Serialize;

/// Token usage from a single LLM call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    /// Tokens served from the provider's prompt cache (Anthropic).
    pub cache_read_tokens: i64,
    /// Tokens written into the provider's prompt cache (Anthropic).
    pub cache_write_tokens: i64,
}

impl TokenUsage {
    pub fn new(prompt: i64, completion: i64) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    /// Attach cache token counts (Anthropic prompt caching).
    pub fn with_cache(mut self, cache_read: i64, cache_write: i64) -> Self {
        self.cache_read_tokens = cache_read;
        self.cache_write_tokens = cache_write;
        self
    }

    /// Parse from OpenAI-format usage JSON (`prompt_tokens`, `completion_tokens`).
    pub fn from_openai_json(u: &serde_json::Value) -> Self {
        Self::new(
            u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        )
    }

    /// Parse from Anthropic-format usage JSON (`input_tokens`, `output_tokens`, cache fields).
    pub fn from_anthropic_json(u: &serde_json::Value) -> Self {
        Self::new(
            u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        )
        .with_cache(
            u.get("cache_read_input_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            u.get("cache_creation_input_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        )
    }

    /// Fraction of prompt tokens served from cache (0.0–1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        if self.prompt_tokens == 0 {
            return 0.0;
        }
        self.cache_read_tokens as f64 / self.prompt_tokens as f64
    }
}

impl AddAssign<&TokenUsage> for TokenUsage {
    fn add_assign(&mut self, rhs: &TokenUsage) {
        self.prompt_tokens += rhs.prompt_tokens;
        self.completion_tokens += rhs.completion_tokens;
        self.total_tokens += rhs.total_tokens;
        self.cache_read_tokens += rhs.cache_read_tokens;
        self.cache_write_tokens += rhs.cache_write_tokens;
    }
}
