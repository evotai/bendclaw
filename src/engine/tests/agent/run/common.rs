//! Test helpers shared across agent-run test modules.

use evotengine::provider::mock::MockProvider;
use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use evotengine::AgentTool;
use evotengine::Content;
use evotengine::ToolContext;
use evotengine::ToolError;
use evotengine::ToolResult;
use evotengine::*;

/// A tool that emits progress updates via `on_update` callback.
pub(super) struct ProgressTool;

#[async_trait::async_trait]
impl AgentTool for ProgressTool {
    fn name(&self) -> &str {
        "progress_tool"
    }
    fn label(&self) -> &str {
        "Progress"
    }
    fn description(&self) -> &str {
        "A tool that streams progress"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        for i in 1..=3 {
            if let Some(ref cb) = ctx.on_update {
                cb(ToolResult {
                    content: vec![Content::Text {
                        text: format!("step {}/3", i),
                    }],
                    details: serde_json::Value::Null,
                    retention: Retention::Normal,
                });
            }
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

/// A provider that fails N times with a given error, then delegates to a
/// `MockProvider`.
pub(super) struct FailThenSucceedProvider {
    pub fail_count: std::sync::atomic::AtomicUsize,
    pub max_failures: usize,
    pub error: ProviderError,
    pub inner: MockProvider,
}

#[async_trait::async_trait]
impl StreamProvider for FailThenSucceedProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let attempt = self
            .fail_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if attempt < self.max_failures {
            return Err(match &self.error {
                ProviderError::RateLimited { message } => ProviderError::RateLimited {
                    message: message.clone(),
                },
                ProviderError::QuotaLimited { message } => ProviderError::QuotaLimited {
                    message: message.clone(),
                },
                ProviderError::Network(msg) => ProviderError::Network(msg.clone()),
                ProviderError::ProtocolIncomplete(msg) => {
                    ProviderError::ProtocolIncomplete(msg.clone())
                }
                ProviderError::Transient { message } => ProviderError::Transient {
                    message: message.clone(),
                },
                ProviderError::Auth(msg) => ProviderError::Auth(msg.clone()),
                other => ProviderError::Other(other.to_string()),
            });
        }
        self.inner.stream(config, tx, cancel).await
    }
}
