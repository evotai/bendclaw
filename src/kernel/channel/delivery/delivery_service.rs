use std::sync::Arc;

use crate::base::Result;
use crate::kernel::channel::delivery::outbound::deliver_outbound;
use crate::kernel::channel::delivery::outbound::OutboundResult;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::delivery::retry::send_with_retry;
use crate::kernel::channel::delivery::retry::RetryConfig;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::session::runtime::session_stream::Stream;

/// Unified channel delivery entry point.
/// Both inbound conversation pipeline and task delivery route through here.
pub struct ChannelDeliveryService;

impl ChannelDeliveryService {
    /// Deliver a run stream to a channel (consume stream, split if needed, retry).
    #[allow(clippy::too_many_arguments)]
    pub async fn deliver_stream(
        outbound: &Arc<dyn ChannelOutbound>,
        rate_limiter: &OutboundRateLimiter,
        channel_type: &str,
        account_id: &str,
        channel_config: &serde_json::Value,
        chat_id: &str,
        max_message_len: usize,
        run_stream: Stream,
    ) -> Result<Option<OutboundResult>> {
        deliver_outbound(
            outbound,
            rate_limiter,
            channel_type,
            account_id,
            channel_config,
            chat_id,
            max_message_len,
            run_stream,
        )
        .await
    }

    /// Deliver plain text to a channel with retry.
    pub async fn deliver_text(
        outbound: &Arc<dyn ChannelOutbound>,
        channel_config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let ob = outbound.clone();
        let cfg = channel_config.clone();
        let cid = chat_id.to_string();
        let t = text.to_string();
        let retry_cfg = RetryConfig::default();

        send_with_retry(
            || {
                let ob = ob.clone();
                let cfg = cfg.clone();
                let cid = cid.clone();
                let t = t.clone();
                async move { ob.send_text(&cfg, &cid, &t).await }
            },
            &retry_cfg,
        )
        .await
    }
}
