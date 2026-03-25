use std::sync::Arc;

use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::delivery::retry::send_with_retry;
use crate::kernel::channel::delivery::retry::RetryConfig;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::session::session_stream::Stream;
use crate::observability::log::slog;
use crate::observability::server_log;

/// Result of outbound delivery: full text + last platform message ID.
pub struct OutboundResult {
    pub text: String,
    pub platform_message_id: String,
}

/// Deliver only the final assistant text to an external channel.
///
/// The run stream may include intermediate assistant text from tool-use turns.
/// Those deltas are internal runtime details and must not be sent to users.
#[allow(clippy::too_many_arguments)]
pub async fn deliver_outbound(
    outbound: &Arc<dyn ChannelOutbound>,
    rate_limiter: &OutboundRateLimiter,
    channel_type: &str,
    account_id: &str,
    channel_config: &serde_json::Value,
    chat_id: &str,
    max_message_len: usize,
    run_stream: Stream,
) -> crate::base::Result<Option<OutboundResult>> {
    let run_id = run_stream.run_id().to_string();
    let finished = run_stream.finish_output().await?;
    let text = finished.text;
    if text.trim().is_empty() {
        return Ok(None);
    }

    slog!(info, "channel", "final_output",
        msg = "channel final output ready",
        run_id = %run_id,
        channel_type,
        account_id,
        chat_id,
        output_preview = %server_log::preview_text(&text),
        output_bytes = text.len() as u64,
    );

    let chunks = split_text_for_delivery(&text, max_message_len.max(1));
    let mut last_msg_id = String::new();

    for chunk in chunks {
        rate_limiter.wait_if_needed(channel_type, account_id).await;
        last_msg_id = send_text_chunk(outbound, channel_config, chat_id, chunk).await?;
    }

    Ok(Some(OutboundResult {
        text,
        platform_message_id: last_msg_id,
    }))
}

async fn send_text_chunk(
    outbound: &Arc<dyn ChannelOutbound>,
    channel_config: &serde_json::Value,
    chat_id: &str,
    text: String,
) -> crate::base::Result<String> {
    let ob = outbound.clone();
    let cfg = channel_config.clone();
    let cid = chat_id.to_string();
    let retry_cfg = RetryConfig::default();

    send_with_retry(
        || {
            let ob = ob.clone();
            let cfg = cfg.clone();
            let cid = cid.clone();
            let text = text.clone();
            async move { ob.send_text(&cfg, &cid, &text).await }
        },
        &retry_cfg,
    )
    .await
}

fn split_text_for_delivery(text: &str, max_bytes: usize) -> Vec<String> {
    if text.is_empty() || max_bytes == 0 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = text.floor_char_boundary((start + max_bytes).min(text.len()));
        if end == start {
            break;
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::split_text_for_delivery;

    #[test]
    fn split_text_preserves_unicode_boundaries() {
        let chunks = split_text_for_delivery("hello世界", 6);
        assert_eq!(chunks, vec!["hello".to_string(), "世界".to_string()]);
    }

    #[test]
    fn split_text_returns_single_chunk_when_short_enough() {
        let chunks = split_text_for_delivery("hello", 32);
        assert_eq!(chunks, vec!["hello".to_string()]);
    }

    #[test]
    fn split_text_handles_empty_input() {
        let chunks = split_text_for_delivery("", 32);
        assert!(chunks.is_empty());
    }
}
