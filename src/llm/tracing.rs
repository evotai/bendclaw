use std::sync::Arc;

use async_trait::async_trait;
use tokio_stream::StreamExt;

use super::message::ChatMessage;
use super::provider::LLMProvider;
use super::provider::LLMResponse;
use super::stream::ResponseStream;
use super::stream::StreamEvent;
use super::tool::ToolSchema;
use crate::observability::log::slog;
use crate::types::Result;

pub struct TracingProvider {
    inner: Arc<dyn LLMProvider>,
    slot_name: String,
    provider_name: String,
}

impl TracingProvider {
    pub fn wrap(
        inner: Arc<dyn LLMProvider>,
        slot_name: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            slot_name: slot_name.into(),
            provider_name: provider_name.into(),
        }
    }
}

#[async_trait]
impl LLMProvider for TracingProvider {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse> {
        let provider_call_id = format!("provider_{}", crate::types::new_id());

        slog!(info, "llm", "provider_request",
            provider_call_id = %provider_call_id,
            slot_name = %self.slot_name,
            provider_name = %self.provider_name,
            model = %model,
            temperature,
            message_count = messages.len() as u64,
            tool_count = tools.len() as u64,
        );

        let result = self.inner.chat(model, messages, tools, temperature).await;

        match &result {
            Ok(response) => {
                slog!(info, "llm", "provider_response",
                    provider_call_id = %provider_call_id,
                    slot_name = %self.slot_name,
                    provider_name = %self.provider_name,
                    model = %response.model.clone().unwrap_or_else(|| model.to_string()),
                    finish_reason = %response.finish_reason.clone().unwrap_or_default(),
                    has_content = response.content.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
                    tool_call_count = response.tool_calls.len() as u64,
                );
            }
            Err(error) => {
                slog!(warn, "llm", "provider_response_failed",
                    provider_call_id = %provider_call_id,
                    slot_name = %self.slot_name,
                    provider_name = %self.provider_name,
                    model = %model,
                    error = %error,
                );
            }
        }

        result
    }

    fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream {
        let provider_call_id = format!("provider_{}", crate::types::new_id());

        slog!(info, "llm", "provider_stream_request",
            provider_call_id = %provider_call_id,
            slot_name = %self.slot_name,
            provider_name = %self.provider_name,
            model = %model,
            temperature,
            message_count = messages.len() as u64,
            tool_count = tools.len() as u64,
        );

        let inner_stream = self.inner.chat_stream(model, messages, tools, temperature);
        let (writer, stream) = ResponseStream::channel(64);
        let slot_name = self.slot_name.clone();
        let provider_name = self.provider_name.clone();
        let model_name = model.to_string();

        crate::types::spawn_fire_and_forget("llm_tracing_stream", async move {
            let mut inner_stream = inner_stream;
            let mut text = String::new();
            let mut thinking = String::new();
            let mut tool_calls = Vec::new();
            let mut content_events = 0u32;
            let mut thinking_events = 0u32;
            let mut tool_start_events = 0u32;
            let mut tool_delta_events = 0u32;
            let mut tool_end_events = 0u32;
            let mut usage_events = 0u32;
            let mut done_events = 0u32;
            let mut error_events = 0u32;
            let mut finish_reason = String::new();
            let mut response_model = String::new();

            while let Some(event) = inner_stream.next().await {
                match &event {
                    StreamEvent::ContentDelta(chunk) => {
                        content_events += 1;
                        text.push_str(chunk);
                    }
                    StreamEvent::ThinkingDelta(chunk) => {
                        thinking_events += 1;
                        thinking.push_str(chunk);
                    }
                    StreamEvent::ToolCallStart { .. } => {
                        tool_start_events += 1;
                    }
                    StreamEvent::ToolCallDelta { .. } => {
                        tool_delta_events += 1;
                    }
                    StreamEvent::ToolCallEnd {
                        id,
                        name,
                        arguments,
                        ..
                    } => {
                        tool_end_events += 1;
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "name": name,
                            "arguments": arguments,
                        }));
                    }
                    StreamEvent::Usage(_) => {
                        usage_events += 1;
                    }
                    StreamEvent::Done {
                        finish_reason: reason,
                        provider: _,
                        model,
                    } => {
                        done_events += 1;
                        finish_reason = reason.clone();
                        response_model = model.clone().unwrap_or_default();
                    }
                    StreamEvent::Error(_) => {
                        error_events += 1;
                    }
                }
                writer.send(event).await;
            }

            slog!(info, "llm", "provider_stream_completed",
                provider_call_id = %provider_call_id,
                slot_name = %slot_name,
                provider_name = %provider_name,
                model = %if response_model.is_empty() { model_name } else { response_model },
                finish_reason = %finish_reason,
                has_text = !text.is_empty(),
                has_thinking = !thinking.is_empty(),
                tool_call_count = tool_calls.len() as u64,
                stream_event_summary = %format!(
                    "content:{content_events},thinking:{thinking_events},tool_start:{tool_start_events},tool_delta:{tool_delta_events},tool_end:{tool_end_events},usage:{usage_events},done:{done_events},error:{error_events}"
                ),
            );
        });

        stream
    }

    fn pricing(&self, model: &str) -> Option<(f64, f64)> {
        self.inner.pricing(model)
    }

    fn default_model(&self) -> &str {
        self.inner.default_model()
    }

    fn default_temperature(&self) -> f64 {
        self.inner.default_temperature()
    }
}
