/// Mask API key to show only last 4 characters.
pub fn mask_api_key(key: &str) -> String {
    let len = key.len();
    if len <= 4 {
        "*".repeat(len)
    } else {
        format!("***{}", &key[len - 4..])
    }
}

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

pub fn response_headers_value(headers: &HeaderMap) -> Value {
    let mut obj = serde_json::Map::new();
    for (name, value) in headers.iter() {
        obj.insert(
            name.as_str().to_string(),
            Value::String(value.to_str().unwrap_or("<binary>").to_string()),
        );
    }
    Value::Object(obj)
}

pub fn response_request_id(headers: &HeaderMap) -> String {
    for key in [
        "x-request-id",
        "request-id",
        "openai-request-id",
        "anthropic-request-id",
        "x-amzn-requestid",
    ] {
        if let Some(value) = headers.get(key).and_then(|value| value.to_str().ok()) {
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    String::new()
}

use super::message::ChatMessage;
use super::stream::ResponseStream;
use super::tool::ToolSchema;
use super::usage::TokenUsage;
use crate::base::Result;

/// Parsed response from a non-streaming LLM call.
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<super::message::ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    pub model: Option<String>,
}

impl LLMResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// The core abstraction for any LLM backend.
///
/// Every provider implements both `chat` (blocking) and `chat_stream` (incremental).
/// The streaming path is the primary interface — `chat` exists as a convenience
/// for simple one-shot calls.
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// One-shot call: send messages, wait for the full response.
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse>;

    /// Streaming call: returns a `ResponseStream` that yields events as they arrive.
    ///
    /// The provider spawns a background task that pushes `StreamEvent`s into the
    /// stream. Dropping the `ResponseStream` cancels the background work.
    fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream;

    /// Return pricing for the given model as `(input_price, output_price)` in USD per 1M tokens.
    /// Returns `None` if pricing is not configured for this provider.
    fn pricing(&self, _model: &str) -> Option<(f64, f64)> {
        None
    }

    /// Primary model name (for logging / reporting).
    fn default_model(&self) -> &str {
        "unknown"
    }

    /// Default sampling temperature for the primary model.
    fn default_temperature(&self) -> f64 {
        0.7
    }
}
