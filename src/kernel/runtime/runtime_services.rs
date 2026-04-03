use std::sync::Arc;

use crate::kernel::channels::egress::rate_limit::OutboundRateLimiter;
use crate::kernel::channels::egress::rate_limit::RateLimitConfig;
use crate::kernel::channels::ingress::dispatch_debounced;
use crate::kernel::channels::routing::chat_router::ChatHandler;
use crate::kernel::channels::routing::chat_router::ChatRouter;
use crate::kernel::channels::routing::chat_router::ChatRouterConfig;
use crate::kernel::channels::routing::debouncer::DebounceConfig;
use crate::kernel::channels::runtime::supervisor::ChannelSupervisor;
use crate::kernel::runtime::runtime::Runtime;
use crate::kernel::trace::TraceWriter;

pub fn build_channel_registry(
) -> crate::kernel::channels::runtime::channel_registry::ChannelRegistry {
    use crate::kernel::channels::adapters::feishu::FeishuChannel;
    use crate::kernel::channels::adapters::github::GitHubChannel;
    use crate::kernel::channels::adapters::http_api::HttpApiChannel;
    use crate::kernel::channels::adapters::telegram::TelegramChannel;

    let mut registry = crate::kernel::channels::runtime::channel_registry::ChannelRegistry::new();
    registry.register(Arc::new(HttpApiChannel::new()));
    registry.register(Arc::new(TelegramChannel::new()));
    registry.register(Arc::new(FeishuChannel::new()));
    registry.register(Arc::new(GitHubChannel::new()));
    registry
}

pub fn spawn_writers() -> RuntimeWriters {
    RuntimeWriters {
        trace_writer: TraceWriter::spawn(),
        persist_writer: crate::execution::persist::persist_op::spawn_persist_writer(),
        channel_message_writer: crate::kernel::channels::spawn_channel_message_writer(),
        tool_writer: crate::kernel::writer::tool_op::spawn_tool_writer(),
        rate_limiter: Arc::new(OutboundRateLimiter::new(RateLimitConfig::default())),
    }
}

pub struct RuntimeWriters {
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::execution::persist::persist_op::PersistWriter,
    pub channel_message_writer: crate::kernel::channels::ChannelMessageWriter,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub rate_limiter: Arc<OutboundRateLimiter>,
}

pub fn build_chat_router(weak: &std::sync::Weak<Runtime>) -> Arc<ChatRouter> {
    let weak_for_handler = weak.clone();
    let handler: ChatHandler = Arc::new(move |input| {
        let weak = weak_for_handler.clone();
        Box::pin(async move {
            if let Some(runtime) = weak.upgrade() {
                dispatch_debounced(&runtime, input).await;
            }
        })
    });
    Arc::new(ChatRouter::new(
        ChatRouterConfig::default(),
        DebounceConfig::default(),
        handler,
    ))
}

pub fn build_supervisor(
    channels: Arc<crate::kernel::channels::runtime::channel_registry::ChannelRegistry>,
    chat_router: Arc<ChatRouter>,
) -> Arc<ChannelSupervisor> {
    let channel_status = Arc::new(crate::kernel::channels::model::status::ChannelStatus::new());
    Arc::new(ChannelSupervisor::new(
        channels,
        chat_router,
        channel_status,
    ))
}
