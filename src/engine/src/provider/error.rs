//! Provider error types and classification.

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Rate limited, retry after {retry_after_ms:?}ms")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("Context overflow: {message}")]
    ContextOverflow { message: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl ProviderError {
    /// Classify an HTTP error response into the appropriate variant.
    pub fn classify(status: u16, message: &str, retry_after_ms: Option<u64>) -> Self {
        if is_context_overflow(status, message) {
            Self::ContextOverflow {
                message: message.to_string(),
            }
        } else if status == 429 {
            Self::RateLimited { retry_after_ms }
        } else if status == 529 || is_overloaded_message(message) {
            Self::Api(message.to_string())
        } else if status == 401 || status == 403 {
            Self::Auth(message.to_string())
        } else if status == 400 || status == 404 || status == 405 || status == 422 {
            Self::Other(message.to_string())
        } else {
            Self::Api(message.to_string())
        }
    }

    pub fn is_context_overflow(&self) -> bool {
        matches!(self, Self::ContextOverflow { .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited {
                retry_after_ms: Some(ms),
            } => Some(Duration::from_millis(*ms)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SSE / eventsource classification
// ---------------------------------------------------------------------------

pub fn classify_sse_error_event(message: &str) -> ProviderError {
    if is_context_overflow_message(message) {
        ProviderError::ContextOverflow {
            message: message.to_string(),
        }
    } else {
        ProviderError::Api(message.to_string())
    }
}

pub async fn classify_eventsource_error(error: reqwest_eventsource::Error) -> ProviderError {
    match error {
        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
            let status_code = status.as_u16();
            let retry_after_ms = super::stream_http::parse_retry_after_header(&response);
            let body = response.text().await.unwrap_or_default();
            ProviderError::classify(
                status_code,
                &format!(
                    "HTTP {} {}: {}",
                    status_code,
                    status.canonical_reason().unwrap_or(""),
                    body
                ),
                retry_after_ms,
            )
        }
        reqwest_eventsource::Error::InvalidContentType(_content_type, response) => {
            let body = response.text().await.unwrap_or_default();
            if body.trim().is_empty() {
                ProviderError::Api("Server returned non-SSE content type".into())
            } else {
                match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(value) => super::stream_http::classify_json_error(&value),
                    Err(_) => ProviderError::classify(200, &body, None),
                }
            }
        }
        reqwest_eventsource::Error::Transport(e) => {
            let mut detail = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                detail.push_str(" -> ");
                detail.push_str(&cause.to_string());
                source = cause.source();
            }
            ProviderError::Network(detail)
        }
        other => ProviderError::Other(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Context overflow detection
// ---------------------------------------------------------------------------

const OVERFLOW_PHRASES: &[&str] = &[
    "prompt is too long",                 // Anthropic
    "input is too long",                  // AWS Bedrock
    "exceeds the context window",         // OpenAI (Completions & Responses)
    "exceeds the maximum",                // Google Gemini
    "maximum prompt length",              // xAI
    "reduce the length of the messages",  // Groq
    "maximum context length",             // OpenRouter
    "exceeds the limit of",               // GitHub Copilot
    "exceeds the available context size", // llama.cpp
    "greater than the context length",    // LM Studio
    "context window exceeds limit",       // MiniMax
    "exceeded model token limit",         // Kimi
    "context length exceeded",            // Generic
    "context_length_exceeded",            // Generic (underscore variant)
    "too many tokens",                    // Generic
    "token limit exceeded",               // Generic
];

pub fn is_context_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    OVERFLOW_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

fn is_context_overflow(status: u16, message: &str) -> bool {
    if (status == 400 || status == 413) && message.trim().is_empty() {
        return true;
    }
    is_context_overflow_message(message)
}

fn is_overloaded_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("overloaded_error") || lower.contains("service is overloaded")
}
