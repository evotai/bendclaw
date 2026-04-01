use std::sync::Arc;

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
