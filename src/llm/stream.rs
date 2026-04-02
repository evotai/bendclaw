use std::pin::Pin;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

use super::usage::TokenUsage;

/// A single event from a streaming LLM response.
///
/// Events arrive in this order:
///   ContentDelta* | ThinkingDelta* | ToolCall sequences → Usage? → Done
///
/// A tool call sequence: `ToolCallStart → ToolCallDelta* → ToolCallEnd`
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of assistant text.
    ContentDelta(String),

    /// A chunk of reasoning/thinking (extended thinking models).
    ThinkingDelta(String),

    /// A new tool call begins.
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },

    /// Incremental JSON for an in-progress tool call.
    ToolCallDelta { index: usize, json_chunk: String },

    /// A tool call is fully received.
    ToolCallEnd {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },

    /// Token usage for this response.
    Usage(TokenUsage),

    /// Stream completed normally.
    Done {
        finish_reason: String,
        provider: Option<String>,
        model: Option<String>,
    },

    /// Something went wrong; stream closes after this.
    Error(String),
}

/// A live stream of LLM events. Consumers iterate with `StreamExt::next()`.
pub struct ResponseStream {
    inner: ReceiverStream<StreamEvent>,
}

impl ResponseStream {
    /// Create a (writer, stream) pair. The writer pushes; the stream yields.
    pub fn channel(buffer: usize) -> (StreamWriter, Self) {
        let (tx, rx) = mpsc::channel(buffer);
        (StreamWriter { tx }, Self {
            inner: ReceiverStream::new(rx),
        })
    }

    /// Create a stream that immediately yields an error event.
    pub fn from_error(e: crate::types::ErrorCode) -> Self {
        let (tx, rx) = mpsc::channel::<StreamEvent>(1);
        // Send error and drop tx to close
        let _ = tx.try_send(StreamEvent::Error(format!("{}", e)));
        Self {
            inner: ReceiverStream::new(rx),
        }
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

/// The write half. Held by the provider; dropping it closes the stream.
#[derive(Clone)]
pub struct StreamWriter {
    tx: mpsc::Sender<StreamEvent>,
}

impl StreamWriter {
    pub async fn send(&self, event: StreamEvent) {
        let _ = self.tx.send(event).await;
    }

    pub async fn text(&self, chunk: impl Into<String>) {
        self.send(StreamEvent::ContentDelta(chunk.into())).await;
    }

    pub async fn thinking(&self, chunk: impl Into<String>) {
        self.send(StreamEvent::ThinkingDelta(chunk.into())).await;
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

    pub async fn usage(&self, u: TokenUsage) {
        self.send(StreamEvent::Usage(u)).await;
    }

    pub async fn done_with_provider(
        &self,
        reason: impl Into<String>,
        provider: Option<String>,
        model: Option<String>,
    ) {
        self.send(StreamEvent::Done {
            finish_reason: reason.into(),
            provider,
            model,
        })
        .await;
    }

    pub async fn done(&self, reason: impl Into<String>) {
        self.done_with_provider(reason, None, None).await;
    }

    pub async fn error(&self, msg: impl Into<String>) {
        self.send(StreamEvent::Error(msg.into())).await;
    }
}

/// Accumulates streaming tool call fragments into complete calls.
///
/// Replaces ad-hoc `Vec<(String, String, String)>` tuples in provider drivers.
///
/// ```ignore
/// let mut acc = ToolCallAccumulator::new();
/// // on tool_call start:
/// let tc = acc.get_or_create(index);
/// tc.id = id;
/// tc.name = name;
/// // on argument delta:
/// acc.get_or_create(index).arguments.push_str(chunk);
/// // at end:
/// for tc in acc.drain() { writer.tool_end(...) }
/// ```
pub struct ToolCallAccumulator {
    calls: Vec<PendingToolCall>,
}

/// A tool call being assembled from streaming fragments.
pub struct PendingToolCall {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self { calls: Vec::new() }
    }

    /// Get or create the slot at `index`, growing the vec if needed.
    pub fn get_or_create(&mut self, index: usize) -> &mut PendingToolCall {
        while self.calls.len() <= index {
            let i = self.calls.len();
            self.calls.push(PendingToolCall {
                index: i,
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        }
        &mut self.calls[index]
    }

    /// Find a call by index (read-only).
    pub fn find(&self, index: usize) -> Option<&PendingToolCall> {
        self.calls.iter().find(|tc| tc.index == index)
    }

    /// Drain all non-empty tool calls.
    pub fn drain(&mut self) -> Vec<PendingToolCall> {
        std::mem::take(&mut self.calls)
            .into_iter()
            .filter(|tc| !tc.id.is_empty())
            .collect()
    }
}

impl Default for ToolCallAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
