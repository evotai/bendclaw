use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::reliable::ReliableProvider;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::llm::usage::TokenUsage;
use parking_lot::Mutex;

/// Provider that fails N times then succeeds.
struct FailThenSucceed {
    remaining_failures: Mutex<u32>,
    error_code: u16,
}

impl FailThenSucceed {
    fn new(failures: u32, error_code: u16) -> Self {
        Self {
            remaining_failures: Mutex::new(failures),
            error_code,
        }
    }
}

#[async_trait]
impl LLMProvider for FailThenSucceed {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        let mut remaining = self.remaining_failures.lock();
        if *remaining > 0 {
            *remaining -= 1;
            Err(ErrorCode::new(
                self.error_code,
                "TestError",
                "transient failure",
            ))
        } else {
            Ok(LLMResponse {
                content: Some("success".into()),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: Some(TokenUsage::new(10, 5)),
                model: Some("test".into()),
            })
        }
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let remaining_failures = {
            let mut r = self.remaining_failures.lock();
            if *r > 0 {
                *r -= 1;
                true
            } else {
                false
            }
        };

        let (writer, stream) = ResponseStream::channel(16);
        tokio::spawn(async move {
            if remaining_failures {
                writer.error("transient failure").await;
            } else {
                writer.text("streamed success").await;
                writer.done("stop").await;
            }
        });
        stream
    }
}

/// Provider that always fails.
struct AlwaysFail;

#[async_trait]
impl LLMProvider for AlwaysFail {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        Err(ErrorCode::llm_rate_limit("rate limited"))
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let (writer, stream) = ResponseStream::channel(16);
        tokio::spawn(async move {
            writer.error("rate limited").await;
        });
        stream
    }
}

// ── ReliableProvider chat retries ──

#[tokio::test]
async fn reliable_retries_on_rate_limit_then_succeeds() -> Result<()> {
    let inner = Arc::new(FailThenSucceed::new(2, ErrorCode::LLM_RATE_LIMIT));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    let resp = result?;
    assert_eq!(
        resp.content.ok_or_else(|| anyhow::anyhow!("no content"))?,
        "success"
    );
    Ok(())
}

#[tokio::test]
async fn reliable_retries_on_server_error() {
    let inner = Arc::new(FailThenSucceed::new(1, ErrorCode::LLM_SERVER));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn reliable_retries_on_timeout() {
    let inner = Arc::new(FailThenSucceed::new(1, ErrorCode::TIMEOUT));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn reliable_exhausts_retries() {
    let inner: Arc<dyn LLMProvider> = Arc::new(AlwaysFail);
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(2)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::LLM_RATE_LIMIT);
}

// ── ReliableProvider delegates ──

#[test]
fn reliable_delegates_default_model() {
    let inner = Arc::new(crate::mocks::llm::MockLLMProvider::with_text("hi"));
    let reliable = ReliableProvider::wrap(inner);
    assert_eq!(reliable.default_model(), "unknown");
}

#[test]
fn reliable_delegates_default_temperature() {
    let inner = Arc::new(crate::mocks::llm::MockLLMProvider::with_text("hi"));
    let reliable = ReliableProvider::wrap(inner);
    assert!((reliable.default_temperature() - 0.7).abs() < f64::EPSILON);
}

#[test]
fn reliable_delegates_pricing() {
    let inner = Arc::new(crate::mocks::llm::MockLLMProvider::with_text("hi"));
    let reliable = ReliableProvider::wrap(inner);
    assert!(reliable.pricing("any").is_none());
}

// ── base_backoff_ms clamping ──

#[tokio::test]
async fn reliable_base_backoff_clamped_to_minimum() {
    let inner = Arc::new(FailThenSucceed::new(1, ErrorCode::LLM_RATE_LIMIT));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(2)
        .base_backoff_ms(1); // below MIN_BACKOFF_MS (50)

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_ok());
}

// ── Immediate success (no retries needed) ──

#[tokio::test]
async fn reliable_no_retry_on_success() -> Result<()> {
    let inner = Arc::new(FailThenSucceed::new(0, ErrorCode::LLM_RATE_LIMIT));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(3)
        .base_backoff_ms(50);

    let resp = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await?;
    assert_eq!(
        resp.content.ok_or_else(|| anyhow::anyhow!("no content"))?,
        "success"
    );
    Ok(())
}

// ── Non-retryable errors: should fail immediately without retrying ──

/// Provider that counts how many times `chat` is called.
struct CountingFail {
    call_count: Mutex<u32>,
    error_code: u16,
    message: String,
}

impl CountingFail {
    fn new(error_code: u16, message: impl Into<String>) -> Self {
        Self {
            call_count: Mutex::new(0),
            error_code,
            message: message.into(),
        }
    }

    fn calls(&self) -> u32 {
        *self.call_count.lock()
    }
}

#[async_trait]
impl LLMProvider for CountingFail {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        *self.call_count.lock() += 1;
        Err(ErrorCode::new(self.error_code, "TestError", &self.message))
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        *self.call_count.lock() += 1;
        let msg = self.message.clone();
        let (writer, stream) = ResponseStream::channel(16);
        tokio::spawn(async move {
            writer.error(msg).await;
        });
        stream
    }
}

#[tokio::test]
async fn reliable_no_retry_on_llm_request_error() {
    let inner = Arc::new(CountingFail::new(ErrorCode::LLM_REQUEST, "bad request"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 1, "should not retry non-retryable error");
}

#[tokio::test]
async fn reliable_no_retry_on_llm_parse_error() {
    let inner = Arc::new(CountingFail::new(ErrorCode::LLM_PARSE, "parse failure"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 1);
}

#[tokio::test]
async fn reliable_no_retry_on_internal_error() {
    let inner = Arc::new(CountingFail::new(ErrorCode::INTERNAL, "internal error"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(3)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 1);
}

// ── Message-based retryability ──

#[tokio::test]
async fn reliable_retries_on_rate_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "rate limit exceeded",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(2)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    // 1 initial + 2 retries = 3 calls
    assert_eq!(inner.calls(), 3);
}

#[tokio::test]
async fn reliable_retries_on_overloaded_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(ErrorCode::INTERNAL, "server overloaded"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_retries_on_503_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "upstream returned 503",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_retries_on_502_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(ErrorCode::INTERNAL, "bad gateway 502"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_retries_on_429_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "HTTP 429 too many requests",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_retries_on_timeout_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "request timeout after 30s",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_retries_on_connection_keyword_in_message() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "connection reset by peer",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn reliable_message_match_is_case_insensitive() {
    let inner = Arc::new(CountingFail::new(
        ErrorCode::INTERNAL,
        "Server OVERLOADED please retry",
    ));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(1)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(
        inner.calls(),
        2,
        "case-insensitive match should trigger retry"
    );
}

// ── max_retries(0) means no retries ──

#[tokio::test]
async fn reliable_zero_retries_fails_immediately() {
    let inner = Arc::new(CountingFail::new(ErrorCode::LLM_RATE_LIMIT, "rate limited"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(0)
        .base_backoff_ms(50);

    let result = reliable
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert_eq!(inner.calls(), 1, "max_retries(0) should not retry");
}

// ── chat_stream: zero retries fails immediately ──

#[tokio::test]
async fn reliable_stream_zero_retries_fails_immediately() {
    use tokio_stream::StreamExt;

    let inner = Arc::new(CountingFail::new(ErrorCode::LLM_RATE_LIMIT, "rate limited"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(0)
        .base_backoff_ms(50);

    let mut stream = reliable.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);

    let mut got_error = false;
    while let Some(event) = stream.next().await {
        if let bendclaw::llm::stream::StreamEvent::Error(_) = event {
            got_error = true;
        }
    }
    assert!(got_error, "should get error with zero retries");
    assert_eq!(inner.calls(), 1, "max_retries(0) should not retry stream");
}

// ── chat_stream: always retries on any error (no keyword check) ──

#[tokio::test]
async fn reliable_stream_retries_on_any_error() {
    use tokio_stream::StreamExt;

    // Unlike chat(), stream retries on ANY error regardless of code or message.
    let inner = Arc::new(CountingFail::new(ErrorCode::INTERNAL, "bad input"));
    let reliable = ReliableProvider::wrap(inner.clone())
        .max_retries(2)
        .base_backoff_ms(50);

    let mut stream = reliable.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);

    let mut got_error = false;
    while let Some(event) = stream.next().await {
        if let bendclaw::llm::stream::StreamEvent::Error(_) = event {
            got_error = true;
        }
    }
    assert!(got_error);
    // 1 initial + 2 retries = 3 total calls
    assert_eq!(inner.calls(), 3, "stream retries on any error");
}

// ── chat_stream error message includes attempt count ──

#[tokio::test]
async fn reliable_stream_error_message_includes_attempt_count() {
    use tokio_stream::StreamExt;

    let inner: Arc<dyn LLMProvider> = Arc::new(AlwaysFail);
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(1)
        .base_backoff_ms(50);

    let mut stream = reliable.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);

    let mut error_msg = String::new();
    while let Some(event) = stream.next().await {
        if let bendclaw::llm::stream::StreamEvent::Error(msg) = event {
            error_msg = msg;
        }
    }
    // max_retries=1 means 2 total attempts
    assert!(
        error_msg.contains("failed after 2 attempts"),
        "got: {error_msg}"
    );
}

// ── chat_stream retries ──

#[tokio::test]
async fn reliable_stream_retries_then_succeeds() -> anyhow::Result<()> {
    use tokio_stream::StreamExt;

    let inner = Arc::new(FailThenSucceed::new(2, ErrorCode::LLM_RATE_LIMIT));
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(3)
        .base_backoff_ms(50);

    let mut stream = reliable.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);

    let mut got_text = false;
    let mut got_done = false;
    while let Some(event) = stream.next().await {
        match event {
            bendclaw::llm::stream::StreamEvent::ContentDelta(text) => {
                assert_eq!(text, "streamed success");
                got_text = true;
            }
            bendclaw::llm::stream::StreamEvent::Done { .. } => {
                got_done = true;
            }
            bendclaw::llm::stream::StreamEvent::Error(msg) => {
                anyhow::bail!("unexpected error after retries: {msg}");
            }
            _ => {}
        }
    }
    assert!(got_text, "should have received streamed text");
    assert!(got_done, "should have received done event");
    Ok(())
}

#[tokio::test]
async fn reliable_stream_exhausts_retries() {
    use tokio_stream::StreamExt;

    let inner: Arc<dyn LLMProvider> = Arc::new(AlwaysFail);
    let reliable = ReliableProvider::wrap(inner)
        .max_retries(2)
        .base_backoff_ms(50);

    let mut stream = reliable.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);

    let mut got_error = false;
    while let Some(event) = stream.next().await {
        if let bendclaw::llm::stream::StreamEvent::Error(msg) = event {
            assert!(
                msg.contains("failed after 3 attempts"),
                "error should mention attempt count, got: {msg}"
            );
            got_error = true;
        }
    }
    assert!(
        got_error,
        "should have received error after exhausting retries"
    );
}
