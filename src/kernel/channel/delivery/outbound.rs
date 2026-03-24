use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio_stream::StreamExt;

use crate::base::truncate_bytes_on_char_boundary;
use crate::kernel::channel::delivery::block_coalescer::BlockCoalescer;
use crate::kernel::channel::delivery::fallback::FallbackDelivery;
use crate::kernel::channel::delivery::outbound_queue::OutboundQueue;
use crate::kernel::channel::delivery::outbound_queue::QueuedMessage;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::delivery::retry::send_with_retry;
use crate::kernel::channel::delivery::retry::RetryConfig;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::channel::stream_delivery::StreamDeliveryConfig;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::observability::log::channel_log;

/// Result of outbound delivery: text + platform message ID.
pub struct OutboundResult {
    pub text: String,
    pub platform_message_id: String,
}

pub(crate) struct TypingRefresher {
    last: Instant,
    interval: Duration,
}

impl TypingRefresher {
    pub fn new(interval: Duration) -> Self {
        Self {
            last: Instant::now(),
            interval,
        }
    }

    pub async fn tick(
        &mut self,
        outbound: &dyn ChannelOutbound,
        config: &serde_json::Value,
        chat_id: &str,
    ) {
        if self.last.elapsed() >= self.interval {
            let _ = outbound.send_typing(config, chat_id).await;
            self.last = Instant::now();
        }
    }
}

/// Deliver outbound response to a channel.
///
/// Handles rate limiting, streaming vs non-streaming, fallback, retry,
/// and dead-letter enqueue. Dispatch only needs to call this and record
/// the result.
#[allow(clippy::too_many_arguments)]
pub async fn deliver_outbound<S>(
    outbound: &Arc<dyn ChannelOutbound>,
    rate_limiter: &OutboundRateLimiter,
    outbound_queue: &OutboundQueue,
    channel_type: &str,
    account_id: &str,
    channel_config: &serde_json::Value,
    chat_id: &str,
    supports_edit: bool,
    max_message_len: usize,
    run_stream: &mut S,
) -> crate::base::Result<Option<OutboundResult>>
where
    S: tokio_stream::Stream<Item = Event> + Unpin,
{
    rate_limiter.wait_if_needed(channel_type, account_id).await;

    if supports_edit {
        deliver_streaming(
            outbound,
            channel_config,
            chat_id,
            max_message_len,
            run_stream,
        )
        .await
    } else {
        deliver_non_streaming(
            outbound,
            outbound_queue,
            channel_type,
            account_id,
            channel_config,
            chat_id,
            max_message_len,
            run_stream,
        )
        .await
    }
}

async fn deliver_streaming<S>(
    outbound: &Arc<dyn ChannelOutbound>,
    channel_config: &serde_json::Value,
    chat_id: &str,
    max_message_len: usize,
    run_stream: &mut S,
) -> crate::base::Result<Option<OutboundResult>>
where
    S: tokio_stream::Stream<Item = Event> + Unpin,
{
    let mut refresher = TypingRefresher::new(Duration::from_secs(4));
    let delivery = FallbackDelivery::new(
        StreamDeliveryConfig {
            throttle_ms: 800,
            min_initial_chars: 20,
            max_message_len,
            show_tool_progress: true,
        },
        outbound.clone(),
        channel_config.clone(),
        chat_id.to_string(),
        RetryConfig::default(),
    );

    // Wrap the stream to tick typing indicator on each event before delivery.
    // We collect events and drive delivery manually via a wrapper approach.
    // Since FallbackDelivery takes ownership of the stream, we pre-drain
    // with typing ticks then hand off.
    // Instead, tick typing before calling deliver (once), then let delivery run.
    refresher
        .tick(outbound.as_ref(), channel_config, chat_id)
        .await;

    let result = delivery.deliver(run_stream).await?;
    if result.text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(OutboundResult {
        text: result.text,
        platform_message_id: result.platform_message_id,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn deliver_non_streaming<S>(
    outbound: &Arc<dyn ChannelOutbound>,
    outbound_queue: &OutboundQueue,
    channel_type: &str,
    account_id: &str,
    channel_config: &serde_json::Value,
    chat_id: &str,
    max_message_len: usize,
    run_stream: &mut S,
) -> crate::base::Result<Option<OutboundResult>>
where
    S: tokio_stream::Stream<Item = Event> + Unpin,
{
    let mut coalescer = BlockCoalescer::new(800, 1200);
    let mut refresher = TypingRefresher::new(Duration::from_secs(4));
    let mut all_text = String::new();
    let mut last_msg_id = String::new();

    while let Some(ev) = run_stream.next().await {
        refresher
            .tick(outbound.as_ref(), channel_config, chat_id)
            .await;

        match &ev {
            Event::StreamDelta(Delta::Text { content }) => {
                all_text.push_str(content);
                if let Some(block) = coalescer.push(content) {
                    let block = truncate_bytes_on_char_boundary(&block, max_message_len);
                    last_msg_id = send_block(
                        outbound,
                        outbound_queue,
                        channel_type,
                        account_id,
                        channel_config,
                        chat_id,
                        block,
                    )
                    .await;
                }
            }
            Event::StreamDelta(Delta::ToolCallStart { .. }) | Event::ReasonEnd { .. } => {
                if let Some(block) = coalescer.flush_if_ready() {
                    let block = truncate_bytes_on_char_boundary(&block, max_message_len);
                    last_msg_id = send_block(
                        outbound,
                        outbound_queue,
                        channel_type,
                        account_id,
                        channel_config,
                        chat_id,
                        block,
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    // Send remaining buffer
    let remaining = coalescer.take();
    if !remaining.is_empty() {
        let remaining = truncate_bytes_on_char_boundary(&remaining, max_message_len);
        last_msg_id = send_block(
            outbound,
            outbound_queue,
            channel_type,
            account_id,
            channel_config,
            chat_id,
            remaining,
        )
        .await;
    }

    if all_text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(OutboundResult {
        text: all_text,
        platform_message_id: last_msg_id,
    }))
}

async fn send_block(
    outbound: &Arc<dyn ChannelOutbound>,
    outbound_queue: &OutboundQueue,
    channel_type: &str,
    account_id: &str,
    channel_config: &serde_json::Value,
    chat_id: &str,
    text: String,
) -> String {
    if text.trim().is_empty() {
        return String::new();
    }

    let started = Instant::now();
    let ob = outbound.clone();
    let cfg = channel_config.clone();
    let cid = chat_id.to_string();
    let txt = text.clone();
    let retry_cfg = RetryConfig::default();

    match send_with_retry(
        || {
            let ob = ob.clone();
            let cfg = cfg.clone();
            let cid = cid.clone();
            let txt = txt.clone();
            async move { ob.send_text(&cfg, &cid, &txt).await }
        },
        &retry_cfg,
    )
    .await
    {
        Ok(msg_id) => {
            channel_log!(debug, "outbound", "sent",
                channel_type = %channel_type,
                account_id = %account_id,
                chat_id,
                send_type = "text",
                message_id = %msg_id,
                output_bytes = text.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
            );
            msg_id
        }
        Err(error) => {
            channel_log!(warn, "outbound", "failed_enqueuing",
                channel_type = %channel_type,
                account_id = %account_id,
                chat_id,
                send_type = "text",
                output_bytes = text.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %error,
            );
            outbound_queue.enqueue(QueuedMessage {
                outbound: outbound.clone(),
                config: channel_config.clone(),
                chat_id: chat_id.to_string(),
                text,
                attempt: 1,
                next_attempt_at: Instant::now() + Duration::from_secs(2),
            });
            String::new()
        }
    }
}
