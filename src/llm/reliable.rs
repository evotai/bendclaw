use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use backon::BackoffBuilder;
use backon::ExponentialBuilder;
use backon::Retryable;
use tracing::Instrument as _;

use super::message::ChatMessage;
use super::provider::LLMProvider;
use super::provider::LLMResponse;
use super::stream::ResponseStream;
use super::stream::StreamEvent;
use super::tool::ToolSchema;
use crate::base::ErrorCode;
use crate::base::Result;

const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_BASE_BACKOFF_MS: u64 = 1000;
const MIN_BACKOFF_MS: u64 = 50;
const MAX_BACKOFF_MS: u64 = 10_000;

/// Wraps any `LLMProvider` with exponential-backoff retry.
///
/// Both `chat` and `chat_stream` are retried transparently.
/// Streaming retries restart the entire request — partial results are discarded.
///
/// ```ignore
/// let reliable = ReliableProvider::wrap(inner)
///     .max_retries(5)
///     .base_backoff_ms(500);
/// ```
pub struct ReliableProvider {
    inner: Arc<dyn LLMProvider>,
    max_retries: u32,
    base_backoff_ms: u64,
}

impl ReliableProvider {
    /// Wrap a provider with default retry settings (3 retries, 1s base backoff).
    pub fn wrap(inner: Arc<dyn LLMProvider>) -> Self {
        Self {
            inner,
            max_retries: DEFAULT_MAX_RETRIES,
            base_backoff_ms: DEFAULT_BASE_BACKOFF_MS,
        }
    }

    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn base_backoff_ms(mut self, ms: u64) -> Self {
        self.base_backoff_ms = ms.max(MIN_BACKOFF_MS);
        self
    }

    fn backoff_builder(&self) -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(self.base_backoff_ms))
            .with_max_delay(Duration::from_millis(MAX_BACKOFF_MS))
            .with_max_times(self.max_retries as usize)
            .with_jitter()
    }
}

#[async_trait]
impl LLMProvider for ReliableProvider {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse> {
        let op = || async { self.inner.chat(model, messages, tools, temperature).await };

        op.retry(self.backoff_builder())
            .when(is_retryable)
            .notify(|e: &ErrorCode, dur: Duration| {
                tracing::warn!(model, error = %e, delay_ms = dur.as_millis() as u64, "LLM call failed, retrying");
            })
            .await
    }

    fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream {
        let (writer, stream) = ResponseStream::channel(64);
        let inner = self.inner.clone();
        let max_retries = self.max_retries;
        let mut delays = self.backoff_builder().build();

        let model = model.to_string();
        let messages = messages.to_vec();
        let tools = tools.to_vec();
        let span = tracing::Span::current();

        tokio::spawn(
            async move {
                loop {
                    let mut inner_stream =
                        inner.chat_stream(&model, &messages, &tools, temperature);

                    let mut got_error = false;
                    let mut error_msg = String::new();

                    use tokio_stream::StreamExt;
                    while let Some(event) = inner_stream.next().await {
                        match &event {
                            StreamEvent::Error(msg) => {
                                got_error = true;
                                error_msg = msg.clone();
                                break;
                            }
                            StreamEvent::Done { .. } => {
                                writer.send(event).await;
                                return;
                            }
                            _ => {
                                writer.send(event).await;
                            }
                        }
                    }

                    if !got_error {
                        return;
                    }

                    match delays.next() {
                        Some(delay) => {
                            tracing::warn!(
                                model = %model,
                                delay_ms = delay.as_millis() as u64,
                                error = %error_msg,
                                "LLM stream failed, retrying"
                            );
                            tokio::time::sleep(delay).await;
                        }
                        None => {
                            writer
                                .error(format!(
                                    "LLM stream failed after {} attempts: {error_msg}",
                                    max_retries + 1
                                ))
                                .await;
                            return;
                        }
                    }
                }
            }
            .instrument(span),
        );

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

/// Decide if an error is worth retrying (rate limits, server errors, network).
fn is_retryable(e: &ErrorCode) -> bool {
    matches!(
        e.code,
        ErrorCode::LLM_RATE_LIMIT | ErrorCode::LLM_SERVER | ErrorCode::TIMEOUT
    ) || {
        let msg = e.message.to_lowercase();
        msg.contains("rate")
            || msg.contains("overloaded")
            || msg.contains("503")
            || msg.contains("502")
            || msg.contains("429")
            || msg.contains("timeout")
            || msg.contains("connection")
    }
}
