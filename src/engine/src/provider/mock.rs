//! Mock provider for testing. No real API calls.

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::error::*;
use super::traits::*;
use crate::types::*;

/// A mock response: either plain text or tool calls
#[derive(Debug, Clone)]
pub enum MockResponse {
    Text(String),
    ToolCalls(Vec<MockToolCall>),
}

#[derive(Debug, Clone)]
pub struct MockToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Mock LLM provider for tests. Supply a sequence of responses.
pub struct MockProvider {
    responses: Mutex<Vec<MockResponse>>,
}

impl MockProvider {
    pub fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }

    /// Convenience: provider that always returns the same text
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![MockResponse::Text(text.into())])
    }

    /// Convenience: sequence of text responses
    pub fn texts(texts: Vec<impl Into<String>>) -> Self {
        Self::new(
            texts
                .into_iter()
                .map(|t| MockResponse::Text(t.into()))
                .collect(),
        )
    }
}

#[async_trait]
impl StreamProvider for MockProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        let response = {
            let mut responses = self.responses.lock();
            if responses.is_empty() {
                MockResponse::Text("(no more mock responses)".into())
            } else {
                responses.remove(0)
            }
        };

        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }

        let _ = tx.send(StreamEvent::Start);

        let message = match response {
            MockResponse::Text(text) => {
                let _ = tx.send(StreamEvent::TextDelta {
                    content_index: 0,
                    delta: text.clone(),
                });
                Message::Assistant {
                    content: vec![Content::Text { text }],
                    stop_reason: StopReason::Stop,
                    model: "mock".into(),
                    provider: "mock".into(),
                    usage: Usage::default(),
                    timestamp: now_ms(),
                    error_message: None,
                    response_id: None,
                }
            }
            MockResponse::ToolCalls(calls) => {
                let content: Vec<Content> = calls
                    .iter()
                    .enumerate()
                    .map(|(i, call)| {
                        let id = format!("mock-tool-{}", i);
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            content_index: i,
                            id: id.clone(),
                            name: call.name.clone(),
                        });
                        let _ = tx.send(StreamEvent::ToolCallEnd { content_index: i });
                        Content::ToolCall {
                            id,
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        }
                    })
                    .collect();

                Message::Assistant {
                    content,
                    stop_reason: StopReason::ToolUse,
                    model: "mock".into(),
                    provider: "mock".into(),
                    usage: Usage::default(),
                    timestamp: now_ms(),
                    error_message: None,
                    response_id: None,
                }
            }
        };

        let _ = tx.send(StreamEvent::Done {
            message: message.clone(),
        });
        Ok(message)
    }
}
