//! Test helpers for provider tests.
//!
//! Provides `StreamConfigBuilder` to reduce `StreamConfig` construction boilerplate,
//! and `collect_stream_events` for event collection.

use bendengine::provider::model::ModelConfig;
use bendengine::provider::traits::*;
use bendengine::types::*;

/// Builder for `StreamConfig` with sensible defaults.
pub struct StreamConfigBuilder {
    model: String,
    system_prompt: String,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    thinking_level: ThinkingLevel,
    api_key: String,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model_config: Option<ModelConfig>,
    cache_config: CacheConfig,
}

impl StreamConfigBuilder {
    /// Create a builder with minimal defaults.
    pub fn new() -> Self {
        Self {
            model: "test-model".into(),
            system_prompt: String::new(),
            messages: vec![Message::user("Hello")],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        }
    }

    /// Anthropic-flavored defaults.
    pub fn anthropic() -> Self {
        Self::new()
            .model("claude-sonnet-4-20250514")
            .api_key("test-key")
    }

    /// OpenAI-flavored defaults.
    pub fn openai() -> Self {
        Self::new()
            .model("gpt-4o")
            .model_config(ModelConfig::openai("gpt-4o", "GPT-4o"))
    }

    pub fn model(mut self, model: &str) -> Self {
        self.model = model.into();
        self
    }

    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn thinking(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }

    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn no_max_tokens(mut self) -> Self {
        self.max_tokens = None;
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = Some(config);
        self
    }

    pub fn cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = config;
        self
    }

    pub fn cache_disabled(mut self) -> Self {
        self.cache_config = CacheConfig {
            enabled: false,
            strategy: CacheStrategy::Disabled,
        };
        self
    }

    pub fn build(self) -> StreamConfig {
        StreamConfig {
            model: self.model,
            system_prompt: self.system_prompt,
            messages: self.messages,
            tools: self.tools,
            thinking_level: self.thinking_level,
            api_key: self.api_key,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            model_config: self.model_config,
            cache_config: self.cache_config,
        }
    }
}

/// Collect all `StreamEvent`s from an unbounded receiver.
pub fn collect_stream_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
) -> Vec<StreamEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// Shorthand: create a tool definition.
pub fn tool_def(name: &str, desc: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: serde_json::json!({"type": "object"}),
    }
}

// ---------------------------------------------------------------------------
// SSE mock server helpers (wiremock)
// ---------------------------------------------------------------------------

use bendengine::provider::error::ProviderError;
use bendengine::provider::StreamProvider;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

/// Run a provider against a wiremock server returning SSE events.
/// Returns (Message, Vec<StreamEvent>) or ProviderError.
pub async fn run_provider_sse(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    sse_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("content-type", "text/event-stream")
                .insert_header("cache-control", "no-cache")
                .set_body_raw(sse_body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|msg| (msg, events))
}

/// Run a provider against a wiremock server returning JSON.
pub async fn run_provider_json(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    json_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(json_body.to_string(), "application/json"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|msg| (msg, events))
}

/// Override the base_url in a StreamConfig's model_config to point at the mock server.
/// For Anthropic (no model_config), creates one with the given base_url.
fn override_base_url(mut config: StreamConfig, base_url: &str) -> StreamConfig {
    let mc = config
        .model_config
        .get_or_insert_with(|| ModelConfig::anthropic("test-model", "Test Model"));
    mc.base_url = base_url.to_string();
    config
}

// ---------------------------------------------------------------------------
// SSE event builders — Anthropic format
// ---------------------------------------------------------------------------

pub mod anthropic_sse {
    /// message_start event with usage.
    pub fn message_start(input_tokens: u64, cache_read: u64) -> String {
        format!(
            "event: message_start\ndata: {}",
            serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": input_tokens,
                        "output_tokens": 0,
                        "cache_read_input_tokens": cache_read,
                        "cache_creation_input_tokens": 0
                    }
                }
            })
        )
    }

    /// content_block_start for text.
    pub fn text_block_start(index: u64) -> String {
        format!(
            "event: content_block_start\ndata: {}",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "text", "text": ""}
            })
        )
    }

    /// content_block_delta for text.
    pub fn text_delta(index: u64, text: &str) -> String {
        format!(
            "event: content_block_delta\ndata: {}",
            serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "text_delta", "text": text}
            })
        )
    }

    /// content_block_stop.
    pub fn block_stop(index: u64) -> String {
        format!(
            "event: content_block_stop\ndata: {}",
            serde_json::json!({"type": "content_block_stop", "index": index})
        )
    }

    /// content_block_start for tool_use.
    pub fn tool_block_start(index: u64, id: &str, name: &str) -> String {
        format!(
            "event: content_block_start\ndata: {}",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "tool_use", "id": id, "name": name}
            })
        )
    }

    /// content_block_delta for tool input JSON.
    pub fn tool_input_delta(index: u64, partial_json: &str) -> String {
        format!(
            "event: content_block_delta\ndata: {}",
            serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "input_json_delta", "partial_json": partial_json}
            })
        )
    }

    /// content_block_start for thinking.
    pub fn thinking_block_start(index: u64) -> String {
        format!(
            "event: content_block_start\ndata: {}",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "thinking", "thinking": ""}
            })
        )
    }

    /// content_block_delta for thinking.
    pub fn thinking_delta(index: u64, text: &str) -> String {
        format!(
            "event: content_block_delta\ndata: {}",
            serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "thinking_delta", "thinking": text}
            })
        )
    }

    /// message_delta with stop_reason and output usage.
    pub fn message_delta(stop_reason: &str, output_tokens: u64) -> String {
        format!(
            "event: message_delta\ndata: {}",
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason},
                "usage": {"output_tokens": output_tokens, "input_tokens": 0}
            })
        )
    }

    /// message_stop event.
    pub fn message_stop() -> String {
        "event: message_stop\ndata: {\"type\":\"message_stop\"}".into()
    }

    /// error event.
    pub fn error(error_type: &str, message: &str) -> String {
        format!(
            "event: error\ndata: {}",
            serde_json::json!({
                "type": error_type,
                "message": message
            })
        )
    }

    /// Join events into an SSE body.
    pub fn body(events: Vec<String>) -> String {
        events.join("\n\n") + "\n\n"
    }
}

// ---------------------------------------------------------------------------
// SSE event builders — OpenAI format
// ---------------------------------------------------------------------------

pub mod openai_sse {
    /// A text content delta chunk.
    pub fn text_chunk(text: &str, finish_reason: Option<&str>) -> String {
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "index": 0,
                    "delta": {"content": text},
                    "finish_reason": finish_reason
                }]
            })
        )
    }

    /// A tool call start chunk.
    pub fn tool_call_start(index: u32, id: &str, name: &str) -> String {
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": index,
                            "id": id,
                            "function": {"name": name}
                        }]
                    },
                    "finish_reason": null
                }]
            })
        )
    }

    /// A tool call argument delta chunk.
    pub fn tool_call_args(index: u32, args: &str) -> String {
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": index,
                            "function": {"arguments": args}
                        }]
                    },
                    "finish_reason": null
                }]
            })
        )
    }

    /// A finish chunk with usage.
    pub fn finish_with_usage(
        finish_reason: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) -> String {
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason
                }],
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": prompt_tokens + completion_tokens
                }
            })
        )
    }

    /// [DONE] marker.
    pub fn done() -> String {
        "data: [DONE]".into()
    }

    /// Join events into an SSE body.
    pub fn body(events: Vec<String>) -> String {
        events.join("\n\n") + "\n\n"
    }
}
