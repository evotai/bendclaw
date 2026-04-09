//! OpenAI-compatible streaming and non-streaming response types.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Streaming (SSE chunk) types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct OpenAiChunk {
    #[serde(default)]
    pub choices: Vec<OpenAiChoice>,
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
    #[serde(default)]
    pub error: Option<OpenAiErrorBody>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiErrorBody {
    #[serde(default)]
    pub message: String,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiChoice {
    pub delta: OpenAiDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct OpenAiDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiToolCallDelta {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<OpenAiFunctionDelta>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiFunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiPromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

// ---------------------------------------------------------------------------
// Non-streaming (full JSON completion) types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct OpenAiResponse {
    #[serde(default)]
    pub choices: Vec<OpenAiResponseChoice>,
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponseChoice {
    pub message: OpenAiResponseMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponseToolCall {
    #[serde(default)]
    pub id: String,
    pub function: OpenAiResponseFunction,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponseFunction {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}
