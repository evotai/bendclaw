use std::sync::Arc;

use crate::kernel::channel::chat_router::ChatHandler;
use crate::kernel::channel::chat_router::ChatRouter;
use crate::kernel::channel::chat_router::ChatRouterConfig;
use crate::kernel::channel::debouncer::DebounceConfig;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::delivery::rate_limit::RateLimitConfig;
use crate::kernel::channel::dispatch::dispatch_debounced;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::runtime::runtime_handle::Runtime;
use crate::kernel::trace::TraceWriter;

pub fn build_channel_registry() -> crate::kernel::channel::registry::ChannelRegistry {
    use crate::kernel::channel::plugins::feishu::FeishuChannel;
    use crate::kernel::channel::plugins::github::GitHubChannel;
    use crate::kernel::channel::plugins::http_api::HttpApiChannel;
    use crate::kernel::channel::plugins::telegram::TelegramChannel;

    let mut registry = crate::kernel::channel::registry::ChannelRegistry::new();
    registry.register(Arc::new(HttpApiChannel::new()));
    registry.register(Arc::new(TelegramChannel::new()));
    registry.register(Arc::new(FeishuChannel::new()));
    registry.register(Arc::new(GitHubChannel::new()));
    registry
}

pub fn spawn_writers() -> RuntimeWriters {
    RuntimeWriters {
        trace_writer: TraceWriter::spawn(),
        persist_writer: crate::kernel::run::persist_op::spawn_persist_writer(),
        channel_message_writer: crate::kernel::channel::spawn_channel_message_writer(),
        tool_writer: crate::kernel::writer::tool_op::spawn_tool_writer(),
        rate_limiter: Arc::new(OutboundRateLimiter::new(RateLimitConfig::default())),
    }
}

pub struct RuntimeWriters {
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::kernel::run::persist_op::PersistWriter,
    pub channel_message_writer: crate::kernel::channel::ChannelMessageWriter,
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
    channels: Arc<crate::kernel::channel::registry::ChannelRegistry>,
    chat_router: Arc<ChatRouter>,
) -> Arc<ChannelSupervisor> {
    let channel_status = Arc::new(crate::kernel::channel::status::ChannelStatus::new());
    Arc::new(ChannelSupervisor::new(
        channels,
        chat_router,
        channel_status,
    ))
}
