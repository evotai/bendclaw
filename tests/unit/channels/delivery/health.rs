use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::channels::egress::health::ChannelHealthMonitor;
use bendclaw::channels::egress::health::HealthMonitorConfig;
use bendclaw::channels::model::account::ChannelAccount;
use bendclaw::channels::routing::chat_router::ChatRouter;
use bendclaw::channels::routing::chat_router::ChatRouterConfig;
use bendclaw::channels::routing::debouncer::DebounceConfig;
use bendclaw::channels::runtime::channel_trait::ChannelOutbound;
use bendclaw::channels::runtime::channel_trait::ChannelPlugin;
use bendclaw::channels::runtime::channel_trait::InboundEventSender;
use bendclaw::channels::runtime::channel_trait::InboundKind;
use bendclaw::channels::runtime::channel_trait::ReceiverFactory;
use bendclaw::channels::ChannelCapabilities;
use bendclaw::channels::ChannelKind;
use bendclaw::channels::ChannelRegistry;
use bendclaw::channels::ChannelSupervisor;
use bendclaw::channels::InboundMode;
use bendclaw::types::Result;
use tokio_util::sync::CancellationToken;

struct DyingReceiverFactory;

#[async_trait]
impl ReceiverFactory for DyingReceiverFactory {
    async fn spawn(
        &self,
        _account: &ChannelAccount,
        _event_tx: InboundEventSender,
        _cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        Ok(tokio::spawn(async {}))
    }
}

struct DyingPlugin;

struct NoopOutbound;

#[async_trait]
impl ChannelOutbound for NoopOutbound {
    async fn send_text(&self, _: &serde_json::Value, _: &str, _: &str) -> Result<String> {
        Ok(String::new())
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> Result<()> {
        Ok(())
    }
    async fn edit_message(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    async fn add_reaction(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl ChannelPlugin for DyingPlugin {
    fn channel_type(&self) -> &str {
        "test_dying"
    }
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::WebSocket,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: false,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: 4096,
            stale_event_threshold: None,
        }
    }
    fn validate_config(&self, _: &serde_json::Value) -> Result<()> {
        Ok(())
    }
    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(NoopOutbound)
    }
    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(DyingReceiverFactory))
    }
}

fn make_account() -> ChannelAccount {
    ChannelAccount {
        channel_account_id: "ca_test".into(),
        channel_type: "test_dying".into(),
        external_account_id: "ext1".into(),
        agent_id: "a_test".into(),
        user_id: "u_test".into(),
        config: serde_json::Value::Null,
        enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[tokio::test]
async fn check_once_restarts_dead_receiver() {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(DyingPlugin));
    let registry = Arc::new(registry);

    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig::default(),
        DebounceConfig::default(),
        Arc::new(|_| Box::pin(async {})),
    ));
    let supervisor = Arc::new(ChannelSupervisor::new(
        registry,
        router,
        Arc::new(bendclaw::channels::model::status::ChannelStatus::new()),
    ));

    let account = make_account();
    supervisor.start(&account).await.unwrap();

    // Wait for the dying receiver to finish.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!supervisor.is_alive("ca_test").await);

    let monitor = ChannelHealthMonitor::new(supervisor.clone(), HealthMonitorConfig {
        poll_interval: Duration::from_secs(1),
        restart_cooldown: Duration::ZERO,
        max_restarts: 3,
    });

    let mut restart_counts = HashMap::new();
    let mut last_restart = HashMap::new();
    monitor
        .check_once(&[account], &mut restart_counts, &mut last_restart)
        .await;

    assert_eq!(*restart_counts.get("ca_test").unwrap_or(&0), 1);
}

#[tokio::test]
async fn check_once_respects_max_restarts() {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(DyingPlugin));
    let registry = Arc::new(registry);

    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig::default(),
        DebounceConfig::default(),
        Arc::new(|_| Box::pin(async {})),
    ));
    let supervisor = Arc::new(ChannelSupervisor::new(
        registry,
        router,
        Arc::new(bendclaw::channels::model::status::ChannelStatus::new()),
    ));

    let account = make_account();
    supervisor.start(&account).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let monitor = ChannelHealthMonitor::new(supervisor.clone(), HealthMonitorConfig {
        poll_interval: Duration::from_secs(1),
        restart_cooldown: Duration::ZERO,
        max_restarts: 2,
    });

    let mut restart_counts = HashMap::new();
    restart_counts.insert("ca_test".to_string(), 2); // Already at max.
    let mut last_restart = HashMap::new();

    monitor
        .check_once(&[account], &mut restart_counts, &mut last_restart)
        .await;

    // Should NOT have incremented — max reached.
    assert_eq!(*restart_counts.get("ca_test").unwrap(), 2);
}
