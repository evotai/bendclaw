//! Anthropic Claude provider (Messages API with streaming)

use async_trait::async_trait;
use futures::StreamExt;
use reqwest_eventsource::Event;
use reqwest_eventsource::EventSource;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::warn;

use super::traits::*;
use crate::types::*;

const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider;

#[async_trait]
impl StreamProvider for AnthropicProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        let is_oauth = config.api_key.contains("sk-ant-oat");

        let base_url = config
            .model_config
            .as_ref()
            .map(|mc| mc.base_url.trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| "https://api.anthropic.com".into());

        let is_custom = base_url != "https://api.anthropic.com";
        let url = format!("{}/v1/messages", base_url);

        let body = build_request_body(&config, is_oauth);
        debug!(
            "Anthropic request: model={}, oauth={}, url={}",
            config.model, is_oauth, url
        );

        let client = reqwest::Client::new();
        let mut builder = client.post(&url).header("content-type", "application/json");

        if is_custom {
            // Custom endpoint — Bearer auth, no Anthropic-specific headers
            builder = builder.header("authorization", format!("Bearer {}", config.api_key));
        } else if is_oauth {
            // Official endpoint, OAuth token
            builder = builder
                .header("authorization", format!("Bearer {}", config.api_key))
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14",
                )
                .header("anthropic-dangerous-direct-browser-access", "true")
                .header("user-agent", "claude-cli/2.1.2 (external, cli)")
                .header("x-app", "cli");
        } else {
            // Official endpoint, API key
            builder = builder
                .header("anthropic-version", API_VERSION)
                .header("x-api-key", &config.api_key);
        }

        // Extra headers from model config (e.g. custom auth overrides)
        if let Some(mc) = &config.model_config {
            for (k, v) in &mc.headers {
                builder = builder.header(k, v);
            }
        }

        let request = builder.json(&body);

        let mut es =
            EventSource::new(request).map_err(|e| ProviderError::Network(e.to_string()))?;

        let mut content: Vec<Content> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = StopReason::Stop;

        let _ = tx.send(StreamEvent::Start);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    es.close();
                    return Err(ProviderError::Cancelled);
                }
                event = es.next() => {
                    match event {
                        None => break,
                        Some(Ok(Event::Open)) => {}
                        Some(Ok(Event::Message(msg))) => {
                            match msg.event.as_str() {
                                "message_start" => {
                                    if let Ok(data) = serde_json::from_str::<AnthropicMessageStart>(&msg.data) {
                                        usage.input = data.message.usage.input_tokens;
                                        usage.cache_read = data.message.usage.cache_read_input_tokens;
                                        usage.cache_write = data.message.usage.cache_creation_input_tokens;
                                    }
                                }
                                "content_block_start" => {
                                    if let Ok(data) = serde_json::from_str::<AnthropicContentBlockStart>(&msg.data) {
                                        let idx = data.index as usize;
                                        match data.content_block {
                                            AnthropicContentBlock::Text { .. } => {
                                                while content.len() <= idx {
                                                    content.push(Content::Text { text: String::new() });
                                                }
                                            }
                                            AnthropicContentBlock::Thinking { .. } => {
                                                while content.len() <= idx {
                                                    content.push(Content::Thinking { thinking: String::new(), signature: None });
                                                }
                                            }
                                            AnthropicContentBlock::ToolUse { id, name, .. } => {
                                                while content.len() <= idx {
                                                    content.push(Content::ToolCall {
                                                        id: id.clone(),
                                                        name: name.clone(),
                                                        arguments: serde_json::Value::Object(Default::default()),
                                                    });
                                                }
                                                let _ = tx.send(StreamEvent::ToolCallStart {
                                                    content_index: idx,
                                                    id,
                                                    name,
                                                });
                                            }
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    if let Ok(data) = serde_json::from_str::<AnthropicContentBlockDelta>(&msg.data) {
                                        let idx = data.index as usize;
                                        match data.delta {
                                            AnthropicDelta::TextDelta { text } => {
                                                if let Some(Content::Text { text: ref mut t }) = content.get_mut(idx) {
                                                    t.push_str(&text);
                                                }
                                                let _ = tx.send(StreamEvent::TextDelta {
                                                    content_index: idx,
                                                    delta: text,
                                                });
                                            }
                                            AnthropicDelta::ThinkingDelta { thinking } => {
                                                if let Some(Content::Thinking { thinking: ref mut t, .. }) = content.get_mut(idx) {
                                                    t.push_str(&thinking);
                                                }
                                                let _ = tx.send(StreamEvent::ThinkingDelta {
                                                    content_index: idx,
                                                    delta: thinking,
                                                });
                                            }
                                            AnthropicDelta::InputJsonDelta { partial_json } => {
                                                // Accumulate JSON into a buffer for this tool call
                                                if let Some(Content::ToolCall { ref mut arguments, .. }) = content.get_mut(idx) {
                                                    // Append to string buffer stored in arguments
                                                    // We accumulate the raw JSON string and parse it at content_block_stop
                                                    let buf = arguments
                                                        .as_object_mut()
                                                        .and_then(|o| o.get_mut("__partial_json"))
                                                        .and_then(|v| v.as_str().map(|s| s.to_string()));
                                                    let new_buf = format!("{}{}", buf.unwrap_or_default(), partial_json);
                                                    if let Some(obj) = arguments.as_object_mut() {
                                                        obj.insert("__partial_json".into(), serde_json::Value::String(new_buf));
                                                    }
                                                }
                                                let _ = tx.send(StreamEvent::ToolCallDelta {
                                                    content_index: idx,
                                                    delta: partial_json,
                                                });
                                            }
                                            AnthropicDelta::SignatureDelta { signature } => {
                                                if let Some(Content::Thinking { signature: ref mut s, .. }) = content.get_mut(idx) {
                                                    *s = Some(signature);
                                                }
                                            }
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                                        let idx = data["index"].as_u64().unwrap_or(0) as usize;
                                        // Parse accumulated JSON for tool calls
                                        if let Some(Content::ToolCall { ref mut arguments, .. }) = content.get_mut(idx) {
                                            if let Some(partial) = arguments.as_object()
                                                .and_then(|o| o.get("__partial_json"))
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string())
                                            {
                                                match super::json_repair::try_repair_json(&partial) {
                                                    Ok(parsed) => *arguments = parsed,
                                                    Err(e) => {
                                                        warn!("Failed to parse tool call JSON: {} ({})", partial, e);
                                                        *arguments = serde_json::Value::Object(Default::default());
                                                    }
                                                }
                                            }
                                        }
                                        let _ = tx.send(StreamEvent::ToolCallEnd { content_index: idx });
                                    }
                                }
                                "message_delta" => {
                                    if let Ok(data) = serde_json::from_str::<AnthropicMessageDelta>(&msg.data) {
                                        stop_reason = match data.delta.stop_reason.as_deref() {
                                            Some("tool_use") => StopReason::ToolUse,
                                            Some("max_tokens") => StopReason::Length,
                                            _ => StopReason::Stop,
                                        };
                                        usage.output = data.usage.output_tokens;
                                    }
                                }
                                "message_stop" => break,
                                "ping" => {}
                                "error" => {
                                    let provider_err = classify_sse_error_event(&msg.data);
                                    warn!("Anthropic stream error: {}", provider_err);
                                    return Err(provider_err);
                                }
                                other => {
                                    debug!("Unknown Anthropic event: {}", other);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let provider_err = classify_eventsource_error(e).await;
                            warn!("SSE error: {}", provider_err);
                            return Err(provider_err);
                        }
                    }
                }
            }
        }

        let has_tool_calls = content
            .iter()
            .any(|c| matches!(c, Content::ToolCall { .. }));
        if has_tool_calls {
            stop_reason = StopReason::ToolUse;
        }

        let message = Message::Assistant {
            content,
            stop_reason,
            model: config.model.clone(),
            provider: "anthropic".into(),
            usage,
            timestamp: now_ms(),
            error_message: None,
        };

        let _ = tx.send(StreamEvent::Done {
            message: message.clone(),
        });
        Ok(message)
    }
}

// ---------------------------------------------------------------------------
// Anthropic API request/response types
// ---------------------------------------------------------------------------

fn build_request_body(config: &StreamConfig, is_oauth: bool) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": content_to_anthropic(content),
                }));
            }
            Message::Assistant { content, .. } => {
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": content_to_anthropic(content),
                }));
            }
            Message::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let result_content = if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                    // Multi-content with images: use array format
                    serde_json::json!(content_to_anthropic(content))
                } else {
                    // Text-only: use string shorthand
                    let text = content
                        .iter()
                        .find_map(|c| match c {
                            Content::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    serde_json::json!(text)
                };

                messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": result_content,
                        "is_error": is_error,
                    }],
                }));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Prompt caching — place cache_control breakpoints based on CacheConfig.
    //
    // Anthropic caches the full prefix (tools → system → messages) up to each
    // breakpoint. We use up to 3 breakpoints:
    //   1. System prompt (stable across turns)
    //   2. Last tool definition (tools rarely change)
    //   3. Second-to-last message (conversation history grows, cache the prefix)
    //
    // When caching is disabled or strategy is Disabled, no markers are added.
    // -----------------------------------------------------------------------
    let cache = &config.cache_config;
    let caching_enabled = cache.enabled && cache.strategy != CacheStrategy::Disabled;
    let (cache_system, cache_tools, cache_messages) = match &cache.strategy {
        CacheStrategy::Auto => (true, true, true),
        CacheStrategy::Disabled => (false, false, false),
        CacheStrategy::Manual {
            cache_system,
            cache_tools,
            cache_messages,
        } => (*cache_system, *cache_tools, *cache_messages),
    };

    // Breakpoint 3: scan backwards from second-to-last message to find one with
    // non-empty content to place the cache breakpoint on
    if caching_enabled && cache_messages && messages.len() >= 2 {
        for idx in (0..messages.len() - 1).rev() {
            if let Some(content) = messages[idx]["content"].as_array_mut() {
                if let Some(last_block) = content.last_mut() {
                    let is_empty_text = last_block.get("type").and_then(|t| t.as_str())
                        == Some("text")
                        && last_block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .is_empty();
                    if !is_empty_text {
                        last_block["cache_control"] = serde_json::json!({"type": "ephemeral"});
                        break;
                    }
                }
            }
        }
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "max_tokens": config.max_tokens.unwrap_or(8192),
        "stream": true,
        "messages": messages,
    });

    // Breakpoint 1: system prompt
    if is_oauth {
        let mut system_blocks = vec![serde_json::json!({
            "type": "text",
            "text": "You are Claude Code, Anthropic's official CLI for Claude.",
        })];
        if !config.system_prompt.is_empty() {
            system_blocks.push(serde_json::json!({
                "type": "text",
                "text": config.system_prompt,
            }));
        }
        // Cache the last system block
        if caching_enabled && cache_system {
            if let Some(last) = system_blocks.last_mut() {
                last["cache_control"] = serde_json::json!({"type": "ephemeral"});
            }
        }
        body["system"] = serde_json::json!(system_blocks);
    } else if !config.system_prompt.is_empty() {
        let mut block = serde_json::json!({
            "type": "text",
            "text": config.system_prompt,
        });
        if caching_enabled && cache_system {
            block["cache_control"] = serde_json::json!({"type": "ephemeral"});
        }
        body["system"] = serde_json::json!([block]);
    }

    // Breakpoint 2: last tool definition (tools are stable between turns)
    if !config.tools.is_empty() {
        let mut tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        if caching_enabled && cache_tools {
            if let Some(last_tool) = tools.last_mut() {
                last_tool["cache_control"] = serde_json::json!({"type": "ephemeral"});
            }
        }
        body["tools"] = serde_json::json!(tools);
    }

    if config.thinking_level != ThinkingLevel::Off {
        let budget = match config.thinking_level {
            ThinkingLevel::Minimal => 128,
            ThinkingLevel::Low => 512,
            ThinkingLevel::Medium => 2048,
            ThinkingLevel::High => 8192,
            ThinkingLevel::Off => 0,
        };
        body["thinking"] = serde_json::json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

fn content_to_anthropic(content: &[Content]) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
        .map(|c| match c {
            Content::Text { text } => serde_json::json!({"type": "text", "text": text}),
            Content::Image { data, mime_type } => serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": mime_type, "data": data},
            }),
            Content::Thinking {
                thinking,
                signature,
            } => serde_json::json!({
                "type": "thinking",
                "thinking": thinking,
                "signature": signature.as_deref().unwrap_or(""),
            }),
            Content::ToolCall {
                id,
                name,
                arguments,
            } => serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": arguments,
            }),
        })
        .collect()
}

// Anthropic SSE event types
#[derive(Deserialize)]
struct AnthropicMessageStart {
    message: AnthropicMessageInfo,
}

#[derive(Deserialize)]
struct AnthropicMessageInfo {
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

#[derive(Deserialize)]
struct AnthropicContentBlockStart {
    index: u64,
    content_block: AnthropicContentBlock,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
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
struct AnthropicContentBlockDelta {
    index: u64,
    delta: AnthropicDelta,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::enum_variant_names)]
enum AnthropicDelta {
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
struct AnthropicMessageDelta {
    delta: AnthropicMessageDeltaInner,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicMessageDeltaInner {
    stop_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::traits::ToolDefinition;

    fn make_config(cache: CacheConfig) -> StreamConfig {
        StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![Message::user("Hello"), Message::User {
                content: vec![Content::Text {
                    text: "What is 2+2?".into(),
                }],
                timestamp: 0,
            }],
            tools: vec![ToolDefinition {
                name: "bash".into(),
                description: "Run commands".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: cache,
        }
    }

    #[test]
    fn test_cache_auto_places_all_breakpoints() {
        let body = build_request_body(&make_config(CacheConfig::default()), false);

        // System prompt should have cache_control
        let system = &body["system"][0];
        assert_eq!(system["cache_control"]["type"], "ephemeral");

        // Last tool should have cache_control
        let tools = body["tools"].as_array().unwrap();
        let last_tool = tools.last().unwrap();
        assert_eq!(last_tool["cache_control"]["type"], "ephemeral");

        // Second-to-last message should have cache_control
        let msgs = body["messages"].as_array().unwrap();
        let second_to_last = &msgs[msgs.len() - 2];
        let content = second_to_last["content"].as_array().unwrap();
        let last_block = content.last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_cache_disabled_no_breakpoints() {
        let config = CacheConfig {
            enabled: false,
            strategy: CacheStrategy::Auto,
        };
        let body = build_request_body(&make_config(config), false);

        // System prompt should NOT have cache_control
        let system = &body["system"][0];
        assert!(system.get("cache_control").is_none());

        // Tools should NOT have cache_control
        let tools = body["tools"].as_array().unwrap();
        assert!(tools.last().unwrap().get("cache_control").is_none());

        // Messages should NOT have cache_control on any block
        let msgs = body["messages"].as_array().unwrap();
        for msg in msgs {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    assert!(block.get("cache_control").is_none());
                }
            }
        }
    }

    #[test]
    fn test_cache_manual_system_only() {
        let config = CacheConfig {
            enabled: true,
            strategy: CacheStrategy::Manual {
                cache_system: true,
                cache_tools: false,
                cache_messages: false,
            },
        };
        let body = build_request_body(&make_config(config), false);

        // System: cached
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        // Tools: not cached
        assert!(body["tools"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .get("cache_control")
            .is_none());
        // Messages: not cached
        let msgs = body["messages"].as_array().unwrap();
        let second = &msgs[msgs.len() - 2];
        let content = second["content"].as_array().unwrap();
        assert!(content.last().unwrap().get("cache_control").is_none());
    }

    #[test]
    fn test_usage_cache_hit_rate() {
        let usage = Usage {
            input: 100,
            output: 50,
            cache_read: 900,
            cache_write: 0,
            total_tokens: 1050,
        };
        let rate = usage.cache_hit_rate();
        assert!((rate - 0.9).abs() < 0.001); // 900 / (100 + 900 + 0) = 0.9

        let empty = Usage::default();
        assert_eq!(empty.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_tool_result_with_image() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "".into(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "tc-1".into(),
                        name: "read_file".into(),
                        arguments: serde_json::json!({"path": "test.png"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                },
                Message::ToolResult {
                    tool_call_id: "tc-1".into(),
                    tool_name: "read_file".into(),
                    content: vec![
                        Content::Text {
                            text: "screenshot".into(),
                        },
                        Content::Image {
                            data: "aW1hZ2VkYXRh".into(),
                            mime_type: "image/png".into(),
                        },
                    ],
                    is_error: false,
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig {
                enabled: false,
                strategy: CacheStrategy::Disabled,
            },
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        // The ToolResult message (second message)
        let tool_msg = &msgs[1];
        let tool_result = &tool_msg["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        // content should be an array (not a string) since it has images
        let content = tool_result["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn test_tool_result_text_only_uses_string() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "".into(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "tc-1".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command": "echo hi"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                },
                Message::ToolResult {
                    tool_call_id: "tc-1".into(),
                    tool_name: "bash".into(),
                    content: vec![Content::Text {
                        text: "hello".into(),
                    }],
                    is_error: false,
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig {
                enabled: false,
                strategy: CacheStrategy::Disabled,
            },
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        let tool_result = &msgs[1]["content"][0];
        // Text-only: content should be a plain string
        assert_eq!(tool_result["content"], "hello");
    }

    #[test]
    fn test_content_to_anthropic_filters_empty_text() {
        let content = vec![
            Content::Text { text: "".into() },
            Content::Text {
                text: "hello".into(),
            },
            Content::Text { text: "".into() },
        ];
        let result = content_to_anthropic(&content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["text"], "hello");
    }

    #[test]
    fn test_cache_control_not_set_on_empty_text_block() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![
                Message::User {
                    content: vec![Content::Text {
                        text: "first message".into(),
                    }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text { text: "".into() }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text {
                        text: "last".into(),
                    }],
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        };
        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        // The second-to-last message has only an empty text block which gets filtered,
        // so its content array should be empty
        let second_to_last = &msgs[msgs.len() - 2];
        let content = second_to_last["content"].as_array().unwrap();
        assert!(
            content.is_empty(),
            "empty text blocks should be filtered out"
        );

        // Cache breakpoint should fall back to the first message instead
        let first = &msgs[0];
        let first_content = first["content"].as_array().unwrap();
        let last_block = first_content.last().unwrap();
        assert_eq!(
            last_block["cache_control"]["type"], "ephemeral",
            "cache_control should fall back to an earlier message with content"
        );
    }

    #[test]
    fn test_cache_breakpoint_falls_back_when_second_to_last_is_empty() {
        // When the second-to-last message has only empty text blocks, the cache
        // breakpoint should scan backwards and land on an earlier non-empty message.
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![
                Message::User {
                    content: vec![Content::Text {
                        text: "first message".into(),
                    }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text { text: "".into() }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text {
                        text: "last message".into(),
                    }],
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();

        // cache_control should be placed on the first message (fallback)
        let first_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(
            first_content.last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );
    }
}
