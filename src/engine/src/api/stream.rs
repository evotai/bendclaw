use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

use super::provider::ProviderResponse;
use super::ApiError;
use crate::types::ContentBlock;
use crate::types::Message;
use crate::types::MessageRole;
use crate::types::StreamMetrics;
use crate::types::Usage;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    ContentDelta(String),
    ThinkingDelta(String),
    ThinkingSignature(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        json_chunk: String,
    },
    ToolCallEnd {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
    Usage(Usage),
    Done {
        finish_reason: String,
        provider: Option<String>,
        model: Option<String>,
    },
    Error(String),
}

#[derive(Debug, Default)]
struct StreamMetricsInner {
    request_started_at: Option<Instant>,
    first_chunk_at: Option<Instant>,
    ttfb_ms: Option<u64>,
    ttft_ms: Option<u64>,
    chunk_count: u32,
    bytes_received: u64,
    stream_duration_ms: u64,
}

impl StreamMetricsInner {
    fn set_request_started_at(&mut self, started_at: Instant) {
        if self.request_started_at.is_none() {
            self.request_started_at = Some(started_at);
        }
    }

    fn record_chunk(&mut self, len: usize) {
        let chunk_started_at = self.first_chunk_at.get_or_insert_with(Instant::now);
        if self.ttfb_ms.is_none() {
            if let Some(started_at) = self.request_started_at {
                self.ttfb_ms = Some(started_at.elapsed().as_millis() as u64);
            } else {
                self.ttfb_ms = Some(chunk_started_at.elapsed().as_millis() as u64);
            }
        }
        self.chunk_count += 1;
        self.bytes_received += len as u64;
    }

    fn record_token(&mut self) {
        if self.ttft_ms.is_none() {
            if let Some(started_at) = self.request_started_at {
                self.ttft_ms = Some(started_at.elapsed().as_millis() as u64);
            } else if let Some(first_chunk_at) = self.first_chunk_at {
                self.ttft_ms = Some(first_chunk_at.elapsed().as_millis() as u64);
            }
        }
    }

    fn finish(&mut self) {
        if let Some(started_at) = self.first_chunk_at.or(self.request_started_at) {
            self.stream_duration_ms = started_at.elapsed().as_millis() as u64;
        }
    }

    fn snapshot(&self) -> StreamMetrics {
        StreamMetrics {
            ttfb_ms: self.ttfb_ms,
            ttft_ms: self.ttft_ms,
            stream_duration_ms: self.stream_duration_ms,
            chunk_count: self.chunk_count,
            bytes_received: self.bytes_received,
        }
    }
}

type SharedMetrics = Arc<Mutex<StreamMetricsInner>>;

pub struct ResponseStream {
    inner: ReceiverStream<StreamEvent>,
    metrics: SharedMetrics,
}

impl ResponseStream {
    pub fn channel(buffer: usize) -> (StreamWriter, Self) {
        let (tx, rx) = mpsc::channel(buffer);
        let metrics = Arc::new(Mutex::new(StreamMetricsInner::default()));
        (
            StreamWriter {
                tx,
                metrics: metrics.clone(),
            },
            Self {
                inner: ReceiverStream::new(rx),
                metrics,
            },
        )
    }

    pub fn from_error(error: ApiError) -> Self {
        let (tx, rx) = mpsc::channel(1);
        let metrics = Arc::new(Mutex::new(StreamMetricsInner::default()));
        let _ = tx.try_send(StreamEvent::Error(error.to_string()));
        Self {
            inner: ReceiverStream::new(rx),
            metrics,
        }
    }

    pub fn metrics(&self) -> StreamMetrics {
        self.metrics.lock().snapshot()
    }
}

impl Stream for ResponseStream {
    type Item = StreamEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

#[derive(Clone)]
pub struct StreamWriter {
    tx: mpsc::Sender<StreamEvent>,
    metrics: SharedMetrics,
}

impl StreamWriter {
    pub fn set_request_started_at(&self, started_at: Instant) {
        self.metrics.lock().set_request_started_at(started_at);
    }

    pub fn record_chunk(&self, len: usize) {
        self.metrics.lock().record_chunk(len);
    }

    fn record_token_if_needed(&self, event: &StreamEvent) {
        if matches!(
            event,
            StreamEvent::ContentDelta(_) | StreamEvent::ThinkingDelta(_)
        ) {
            self.metrics.lock().record_token();
        }
    }

    pub fn finish_metrics(&self) {
        self.metrics.lock().finish();
    }

    pub async fn send(&self, event: StreamEvent) {
        self.record_token_if_needed(&event);
        let _ = self.tx.send(event).await;
    }

    pub async fn text(&self, chunk: impl Into<String>) {
        self.send(StreamEvent::ContentDelta(chunk.into())).await;
    }

    pub async fn thinking(&self, chunk: impl Into<String>) {
        self.send(StreamEvent::ThinkingDelta(chunk.into())).await;
    }

    pub async fn thinking_signature(&self, signature: impl Into<String>) {
        self.send(StreamEvent::ThinkingSignature(signature.into()))
            .await;
    }

    pub async fn tool_start(&self, index: usize, id: impl Into<String>, name: impl Into<String>) {
        self.send(StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        })
        .await;
    }

    pub async fn tool_delta(&self, index: usize, json_chunk: impl Into<String>) {
        self.send(StreamEvent::ToolCallDelta {
            index,
            json_chunk: json_chunk.into(),
        })
        .await;
    }

    pub async fn tool_end(
        &self,
        index: usize,
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) {
        self.send(StreamEvent::ToolCallEnd {
            index,
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        })
        .await;
    }

    pub async fn usage(&self, usage: Usage) {
        self.send(StreamEvent::Usage(usage)).await;
    }

    pub async fn done(
        &self,
        finish_reason: impl Into<String>,
        provider: Option<String>,
        model: Option<String>,
    ) {
        self.finish_metrics();
        self.send(StreamEvent::Done {
            finish_reason: finish_reason.into(),
            provider,
            model,
        })
        .await;
    }

    pub async fn error(&self, message: impl Into<String>) {
        self.finish_metrics();
        self.send(StreamEvent::Error(message.into())).await;
    }

    pub async fn emit_response(
        &self,
        response: ProviderResponse,
        provider: Option<String>,
        model: Option<String>,
    ) {
        for (index, block) in response.message.content.iter().enumerate() {
            match block {
                ContentBlock::Text { text } if !text.is_empty() => self.text(text.clone()).await,
                ContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    if !thinking.is_empty() {
                        self.thinking(thinking.clone()).await;
                    }
                    if let Some(signature) = signature {
                        self.thinking_signature(signature.clone()).await;
                    }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    self.tool_start(index, id.clone(), name.clone()).await;
                    let arguments = input.to_string();
                    if !arguments.is_empty() && arguments != "{}" {
                        self.tool_delta(index, arguments.clone()).await;
                    }
                    self.tool_end(index, id.clone(), name.clone(), arguments)
                        .await;
                }
                _ => {}
            }
        }

        self.usage(response.usage).await;
        self.done(
            response
                .stop_reason
                .unwrap_or_else(|| "end_turn".to_string()),
            provider,
            model,
        )
        .await;
    }
}

#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub started_emitted: bool,
}

#[derive(Default)]
pub struct ToolCallAccumulator {
    calls: Vec<PendingToolCall>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(&mut self, index: usize) -> &mut PendingToolCall {
        while self.calls.len() <= index {
            let next_index = self.calls.len();
            self.calls.push(PendingToolCall {
                index: next_index,
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
                started_emitted: false,
            });
        }
        &mut self.calls[index]
    }

    pub fn drain(&mut self) -> Vec<PendingToolCall> {
        std::mem::take(&mut self.calls)
            .into_iter()
            .filter(|call| !call.id.is_empty())
            .collect()
    }
}

#[derive(Default)]
pub struct StreamAccumulator {
    text: String,
    thinking: String,
    thinking_signature: Option<String>,
    tool_calls: ToolCallAccumulator,
    usage: Usage,
    finish_reason: Option<String>,
}

impl StreamAccumulator {
    pub fn apply(&mut self, event: StreamEvent) -> Result<(), ApiError> {
        match event {
            StreamEvent::ContentDelta(delta) => self.text.push_str(&delta),
            StreamEvent::ThinkingDelta(delta) => self.thinking.push_str(&delta),
            StreamEvent::ThinkingSignature(signature) => {
                self.thinking_signature = Some(signature);
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                let call = self.tool_calls.get_or_create(index);
                call.id = id;
                call.name = name;
                call.started_emitted = true;
            }
            StreamEvent::ToolCallDelta { index, json_chunk } => {
                self.tool_calls
                    .get_or_create(index)
                    .arguments
                    .push_str(&json_chunk);
            }
            StreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => {
                let call = self.tool_calls.get_or_create(index);
                call.id = id;
                call.name = name;
                call.arguments = arguments;
                call.started_emitted = true;
            }
            StreamEvent::Usage(usage) => merge_usage(&mut self.usage, &usage),
            StreamEvent::Done { finish_reason, .. } => self.finish_reason = Some(finish_reason),
            StreamEvent::Error(error) => return Err(ApiError::StreamError(error)),
        }

        Ok(())
    }

    pub fn into_provider_response(mut self) -> ProviderResponse {
        let mut content = Vec::new();
        if !self.text.is_empty() {
            content.push(ContentBlock::Text { text: self.text });
        }
        if !self.thinking.is_empty() || self.thinking_signature.is_some() {
            content.push(ContentBlock::Thinking {
                thinking: self.thinking,
                signature: self.thinking_signature,
            });
        }

        for call in self.tool_calls.drain() {
            let input = serde_json::from_str(&call.arguments)
                .unwrap_or(serde_json::Value::String(call.arguments));
            content.push(ContentBlock::ToolUse {
                id: call.id,
                name: call.name,
                input,
            });
        }

        ProviderResponse {
            message: Message {
                role: MessageRole::Assistant,
                content,
            },
            usage: self.usage,
            stop_reason: self.finish_reason,
        }
    }
}

pub async fn collect_response(mut stream: ResponseStream) -> Result<ProviderResponse, ApiError> {
    let mut accumulator = StreamAccumulator::default();

    while let Some(event) = tokio_stream::StreamExt::next(&mut stream).await {
        accumulator.apply(event)?;
    }

    Ok(accumulator.into_provider_response())
}

fn merge_usage(into: &mut Usage, next: &Usage) {
    let is_complete = next.input_tokens > 0 && next.output_tokens > 0;
    if is_complete {
        let existing_cache_creation = into.cache_creation_input_tokens;
        let existing_cache_read = into.cache_read_input_tokens;
        *into = next.clone();
        if into.cache_creation_input_tokens == 0 {
            into.cache_creation_input_tokens = existing_cache_creation;
        }
        if into.cache_read_input_tokens == 0 {
            into.cache_read_input_tokens = existing_cache_read;
        }
        return;
    }

    into.input_tokens += next.input_tokens;
    into.output_tokens += next.output_tokens;
    into.cache_creation_input_tokens += next.cache_creation_input_tokens;
    into.cache_read_input_tokens += next.cache_read_input_tokens;
}
