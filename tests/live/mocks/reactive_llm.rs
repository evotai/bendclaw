use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::base::Result;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::llm::usage::TokenUsage;

type ReactiveHandler =
    dyn Fn(usize, &[ChatMessage], &[ToolSchema], f32) -> Result<LLMResponse> + Send + Sync;

fn mock_usage() -> TokenUsage {
    TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

pub struct ReactiveMockLLMProvider {
    handler: Arc<ReactiveHandler>,
    call_count: AtomicUsize,
}

impl ReactiveMockLLMProvider {
    pub fn new(
        handler: impl Fn(usize, &[ChatMessage], &[ToolSchema], f32) -> Result<LLMResponse>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
            call_count: AtomicUsize::new(0),
        }
    }

    fn next_call_index(&self) -> usize {
        self.call_count.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl LLMProvider for ReactiveMockLLMProvider {
    async fn chat(
        &self,
        _model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse> {
        let call_index = self.next_call_index();
        (self.handler)(call_index, messages, tools, temperature)
    }

    fn chat_stream(
        &self,
        _model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream {
        let call_index = self.next_call_index();
        let (writer, stream) = ResponseStream::channel(16);
        let response = (self.handler)(call_index, messages, tools, temperature);

        tokio::spawn(async move {
            match response {
                Ok(response) => {
                    if let Some(content) = response.content {
                        writer.send(StreamEvent::ContentDelta(content)).await;
                    }

                    for (index, tool_call) in response.tool_calls.into_iter().enumerate() {
                        writer
                            .send(StreamEvent::ToolCallStart {
                                index,
                                id: tool_call.id.clone(),
                                name: tool_call.name.clone(),
                            })
                            .await;
                        writer
                            .send(StreamEvent::ToolCallEnd {
                                index,
                                id: tool_call.id,
                                name: tool_call.name,
                                arguments: tool_call.arguments,
                            })
                            .await;
                    }

                    writer
                        .send(StreamEvent::Usage(
                            response.usage.unwrap_or_else(mock_usage),
                        ))
                        .await;
                    writer
                        .send(StreamEvent::Done {
                            finish_reason: response.finish_reason.unwrap_or_else(|| "stop".into()),
                            provider: Some("mock".into()),
                            model: Some(response.model.unwrap_or_else(|| "mock".into())),
                        })
                        .await;
                }
                Err(error) => {
                    writer.send(StreamEvent::Error(error.to_string())).await;
                }
            }
        });

        stream
    }

    fn default_model(&self) -> &str {
        "mock"
    }

    fn default_temperature(&self) -> f64 {
        0.0
    }
}
