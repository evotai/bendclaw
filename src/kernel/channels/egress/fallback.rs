use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::stream_delivery::StreamDelivery;
use super::stream_delivery::StreamDeliveryConfig;
use crate::execution::event::Event;
use crate::kernel::channels::egress::retry::send_with_retry;
use crate::kernel::channels::egress::retry::RetryConfig;
use crate::kernel::channels::runtime::channel_trait::ChannelOutbound;
use crate::kernel::channels::runtime::diagnostics;
use crate::types::Result;

/// How the message was ultimately delivered.
pub enum DeliveryMethod {
    Streamed,
    FellBack,
    NoOutput,
}

/// Delivery result: (output_text, platform_message_id, method).
pub struct DeliveryResult {
    pub text: String,
    pub platform_message_id: String,
    pub method: DeliveryMethod,
}

/// Wraps StreamDelivery with automatic fallback to send_text on failure.
pub struct FallbackDelivery {
    stream_config: StreamDeliveryConfig,
    outbound: Arc<dyn ChannelOutbound>,
    channel_config: serde_json::Value,
    chat_id: String,
    retry_config: RetryConfig,
}

impl FallbackDelivery {
    pub fn new(
        stream_config: StreamDeliveryConfig,
        outbound: Arc<dyn ChannelOutbound>,
        channel_config: serde_json::Value,
        chat_id: String,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            stream_config,
            outbound,
            channel_config,
            chat_id,
            retry_config,
        }
    }

    /// Try streaming delivery; if send_draft or finalize_draft failed, fall back to send_text.
    pub async fn deliver<S>(&self, stream: &mut S) -> Result<DeliveryResult>
    where S: tokio_stream::Stream<Item = Event> + Unpin {
        let draft_sent = Arc::new(AtomicBool::new(false));
        let finalize_ok = Arc::new(AtomicBool::new(false));
        let tracking = Arc::new(TrackingOutbound {
            inner: self.outbound.clone(),
            draft_sent: draft_sent.clone(),
            finalize_ok: finalize_ok.clone(),
        }) as Arc<dyn ChannelOutbound>;

        let delivery = StreamDelivery::new(
            StreamDeliveryConfig {
                throttle_ms: self.stream_config.throttle_ms,
                min_initial_chars: self.stream_config.min_initial_chars,
                max_message_len: self.stream_config.max_message_len,
                show_tool_progress: self.stream_config.show_tool_progress,
            },
            tracking,
            self.channel_config.clone(),
            self.chat_id.clone(),
        );

        let text = delivery.deliver(stream).await?;

        if text.trim().is_empty() {
            return Ok(DeliveryResult {
                text,
                platform_message_id: String::new(),
                method: DeliveryMethod::NoOutput,
            });
        }

        let sent = draft_sent.load(Ordering::Relaxed);
        let finalized = finalize_ok.load(Ordering::Relaxed);

        if !sent || !finalized {
            if !sent {
                diagnostics::log_channel_send_draft_failed(None::<&crate::types::ErrorCode>);
            } else {
                diagnostics::log_channel_finalize_draft_failed(None::<&crate::types::ErrorCode>);
            }
            return self.fallback_send_text(&text).await;
        }

        Ok(DeliveryResult {
            text,
            platform_message_id: String::new(),
            method: DeliveryMethod::Streamed,
        })
    }
    async fn fallback_send_text(&self, text: &str) -> Result<DeliveryResult> {
        let ob = self.outbound.clone();
        let config = self.channel_config.clone();
        let chat_id = self.chat_id.clone();
        let text_owned = text.to_string();

        let msg_id = send_with_retry(
            || {
                let ob = ob.clone();
                let config = config.clone();
                let chat_id = chat_id.clone();
                let t = text_owned.clone();
                async move { ob.send_text(&config, &chat_id, &t).await }
            },
            &self.retry_config,
        )
        .await?;

        Ok(DeliveryResult {
            text: text.to_string(),
            platform_message_id: msg_id,
            method: DeliveryMethod::FellBack,
        })
    }
}

/// Outbound decorator that tracks send_draft and finalize_draft success.
struct TrackingOutbound {
    inner: Arc<dyn ChannelOutbound>,
    draft_sent: Arc<AtomicBool>,
    finalize_ok: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl ChannelOutbound for TrackingOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        self.inner.send_text(config, chat_id, text).await
    }

    async fn send_typing(&self, config: &serde_json::Value, chat_id: &str) -> Result<()> {
        self.inner.send_typing(config, chat_id).await
    }

    async fn edit_message(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        self.inner.edit_message(config, chat_id, msg_id, text).await
    }

    async fn add_reaction(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        emoji: &str,
    ) -> Result<()> {
        self.inner
            .add_reaction(config, chat_id, msg_id, emoji)
            .await
    }

    async fn send_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let result = self.inner.send_draft(config, chat_id, text).await;
        if result.is_ok() {
            self.draft_sent.store(true, Ordering::Relaxed);
        }
        result
    }

    async fn update_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        self.inner.update_draft(config, chat_id, msg_id, text).await
    }

    async fn finalize_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        let result = self
            .inner
            .finalize_draft(config, chat_id, msg_id, text)
            .await;
        if result.is_ok() {
            self.finalize_ok.store(true, Ordering::Relaxed);
        }
        result
    }
}
