use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::channel::account::ChannelAccount;
use bendclaw::kernel::channel::capabilities::ChannelCapabilities;
use bendclaw::kernel::channel::capabilities::ChannelKind;
use bendclaw::kernel::channel::capabilities::InboundMode;
use bendclaw::kernel::channel::lease::ChannelLeaseResource;
use bendclaw::kernel::channel::plugin::ChannelOutbound;
use bendclaw::kernel::channel::plugin::ChannelPlugin;
use bendclaw::kernel::channel::plugin::InboundKind;
use bendclaw::kernel::channel::plugin::ReceiverFactory;
use bendclaw::kernel::channel::registry::ChannelRegistry;
use bendclaw::kernel::channel::supervisor::ChannelSupervisor;
use bendclaw::kernel::lease::LeaseResource;
use bendclaw::storage::pool::QueryResponse;
use bendclaw::storage::AgentDatabases;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

// ── Fake channel plugin with Receiver inbound ───────────────────────────────

struct FakeReceiverFactory;

#[async_trait]
impl ReceiverFactory for FakeReceiverFactory {
    async fn spawn(
        &self,
        _account: &ChannelAccount,
        _event_tx: bendclaw::kernel::channel::plugin::InboundEventSender,
        cancel: tokio_util::sync::CancellationToken,
    ) -> bendclaw::base::Result<tokio::task::JoinHandle<()>> {
        Ok(tokio::spawn(async move { cancel.cancelled().await }))
    }
}

struct FakeOutbound;

#[async_trait]
impl ChannelOutbound for FakeOutbound {
    async fn send_text(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<String> {
        Ok(String::new())
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn edit_message(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn add_reaction(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
}

struct ReceiverPlugin;

#[async_trait]
impl ChannelPlugin for ReceiverPlugin {
    fn channel_type(&self) -> &str {
        "fake_receiver"
    }
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::Polling,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: false,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: 4096,
        }
    }
    fn validate_config(&self, _: &serde_json::Value) -> bendclaw::base::Result<()> {
        Ok(())
    }
    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FakeOutbound)
    }
    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(FakeReceiverFactory))
    }
}

struct WebhookOnlyPlugin;

#[async_trait]
impl ChannelPlugin for WebhookOnlyPlugin {
    fn channel_type(&self) -> &str {
        "fake_webhook"
    }
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::EventDriven,
            inbound_mode: InboundMode::Webhook,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: false,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: 4096,
        }
    }
    fn validate_config(&self, _: &serde_json::Value) -> bendclaw::base::Result<()> {
        Ok(())
    }
    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FakeOutbound)
    }
    fn inbound(&self) -> InboundKind {
        InboundKind::None
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn account_row(id: &str, channel_type: &str, enabled: bool) -> Vec<serde_json::Value> {
    vec![
        id.to_string(),
        channel_type.to_string(),
        "ext-1".to_string(),
        "agent1".to_string(),
        "user-1".to_string(),
        r#"{"token":"abc"}"#.to_string(),
        if enabled { "1" } else { "0" }.to_string(),
        String::new(), // lease_node_id
        String::new(), // lease_token
        String::new(), // lease_expires_at
        "2026-01-01T00:00:00Z".to_string(),
        "2026-01-01T00:00:00Z".to_string(),
    ]
    .into_iter()
    .map(serde_json::Value::String)
    .collect()
}

fn build_resource(
    query_handler: impl Fn(&str, Option<&str>) -> Result<QueryResponse, String> + Send + Sync + 'static,
) -> (ChannelLeaseResource, Arc<ChannelSupervisor>) {
    let fake = FakeDatabend::new(move |sql, db| {
        query_handler(sql, db).map_err(|e| bendclaw::base::ErrorCode::internal(e))
    });
    let pool = fake.pool();
    let databases = Arc::new(AgentDatabases::new(pool, "test_").unwrap());

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(ReceiverPlugin));
    registry.register(Arc::new(WebhookOnlyPlugin));
    let registry = Arc::new(registry);

    let supervisor = Arc::new(ChannelSupervisor::new(
        registry.clone(),
        Arc::new(|_, _| {}),
    ));

    let resource = ChannelLeaseResource::new(databases, registry, supervisor.clone());
    (resource, supervisor)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn discover_returns_enabled_receiver_accounts_only() {
    let (resource, _) = build_resource(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        // list_by_agent returns 3 accounts:
        // 1. enabled receiver → should be included
        // 2. disabled receiver → should be excluded
        // 3. enabled webhook-only → should be excluded (no Receiver inbound)
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![
                account_row("acct-1", "fake_receiver", true),
                account_row("acct-2", "fake_receiver", false),
                account_row("acct-3", "fake_webhook", true),
            ],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });

    let entries = resource.discover().await.unwrap();

    assert_eq!(entries.len(), 1, "only enabled receiver accounts");
    assert_eq!(entries[0].id, "acct-1");
}

#[tokio::test]
async fn discover_returns_empty_when_no_receiver_accounts() {
    let (resource, _) = build_resource(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![account_row("acct-1", "fake_webhook", true)],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });

    let entries = resource.discover().await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn on_acquired_starts_supervisor() {
    let (resource, supervisor) = build_resource(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        // load() query for on_acquired
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![account_row("acct-1", "fake_receiver", true)],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });

    let pool = {
        let entries = resource.discover().await.unwrap();
        entries[0].pool.clone()
    };

    let entry = bendclaw::kernel::lease::ResourceEntry {
        id: "acct-1".to_string(),
        pool,
        lease_token: Some("tok-1".to_string()),
        lease_node_id: Some("inst-1".to_string()),
        lease_expires_at: None,
        context: String::new(),
        release_fn: None,
    };

    resource.on_acquired(&entry).await.unwrap();
    assert!(
        supervisor.is_alive("acct-1").await,
        "supervisor should be running after on_acquired"
    );
}

#[tokio::test]
async fn on_released_stops_supervisor() {
    let (resource, supervisor) = build_resource(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![account_row("acct-1", "fake_receiver", true)],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });

    let pool = {
        let entries = resource.discover().await.unwrap();
        entries[0].pool.clone()
    };

    let entry = bendclaw::kernel::lease::ResourceEntry {
        id: "acct-1".to_string(),
        pool: pool.clone(),
        lease_token: Some("tok-1".to_string()),
        lease_node_id: Some("inst-1".to_string()),
        lease_expires_at: None,
        context: String::new(),
        release_fn: None,
    };

    resource.on_acquired(&entry).await.unwrap();
    assert!(supervisor.is_alive("acct-1").await);

    resource.on_released("acct-1", &pool).await;
    // Give stop a moment to propagate.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !supervisor.is_alive("acct-1").await,
        "supervisor should be stopped after on_released"
    );
}

#[tokio::test]
async fn is_healthy_reflects_supervisor_state() {
    let (resource, supervisor) = build_resource(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![account_row("acct-1", "fake_receiver", true)],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });

    // Not started → not healthy.
    assert!(!resource.is_healthy("acct-1").await);

    let pool = {
        let entries = resource.discover().await.unwrap();
        entries[0].pool.clone()
    };

    let entry = bendclaw::kernel::lease::ResourceEntry {
        id: "acct-1".to_string(),
        pool,
        lease_token: Some("tok-1".to_string()),
        lease_node_id: Some("inst-1".to_string()),
        lease_expires_at: None,
        context: String::new(),
        release_fn: None,
    };

    resource.on_acquired(&entry).await.unwrap();
    assert!(
        resource.is_healthy("acct-1").await,
        "running receiver should be healthy"
    );

    supervisor.stop("acct-1").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !resource.is_healthy("acct-1").await,
        "stopped receiver should be unhealthy"
    );
}

#[tokio::test]
async fn claim_condition_requires_enabled() {
    let (resource, _) = build_resource(|_, _| Ok(paged_rows(&[], None, None)));
    assert_eq!(resource.claim_condition(), Some("enabled = true"));
}
