//! Anthropic SSE/JSON payload types.

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct AnthropicMessageStart {
    pub message: AnthropicMessageInfo,
}

#[derive(Deserialize)]
pub(crate) struct AnthropicMessageInfo {
    pub usage: AnthropicUsage,
}

#[derive(Deserialize)]
pub(crate) struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

#[derive(Deserialize)]
pub(crate) struct AnthropicContentBlockStart {
    pub index: u64,
    pub content_block: AnthropicContentBlock,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text {
        #[allow(dead_code)]
        text: String,
    },
    #[serde(rename = "thinking")]
    Thinking {
        #[allow(dead_code)]
        thinking: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
}

#[derive(Deserialize)]
pub(crate) struct AnthropicContentBlockDelta {
    pub index: u64,
    pub delta: AnthropicDelta,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum AnthropicDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

#[derive(Deserialize)]
pub(crate) struct AnthropicMessageDelta {
    pub delta: AnthropicMessageDeltaInner,
    pub usage: AnthropicUsage,
}

#[derive(Deserialize)]
pub(crate) struct AnthropicMessageDeltaInner {
    pub stop_reason: Option<String>,
}
