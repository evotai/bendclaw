use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use async_trait::async_trait;
use bendclaw::base::Result as BaseResult;
use bendclaw::kernel::channel::ChannelCapabilities;
use bendclaw::kernel::channel::ChannelKind;
use bendclaw::kernel::channel::ChannelOutbound;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::ChannelRegistry;
use bendclaw::kernel::channel::InboundKind;
use bendclaw::kernel::channel::InboundMode;

// ── TestPlugin: in-memory mock, no I/O ──

pub struct TestPlugin {
    pub type_name: String,
    pub kind: ChannelKind,
}

impl TestPlugin {
    pub fn conversational(name: &str) -> Self {
        Self {
            type_name: name.to_string(),
            kind: ChannelKind::Conversational,
        }
    }

    pub fn event_driven(name: &str) -> Self {
        Self {
            type_name: name.to_string(),
            kind: ChannelKind::EventDriven,
        }
    }
}

#[async_trait]
impl ChannelPlugin for TestPlugin {
    fn channel_type(&self) -> &str {
        &self.type_name
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: self.kind.clone(),
            inbound_mode: InboundMode::Webhook,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: 4096,
        }
    }

    fn validate_config(&self, _config: &serde_json::Value) -> BaseResult<()> {
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(NullOutbound)
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::None
    }
}

struct NullOutbound;

#[async_trait]
impl ChannelOutbound for NullOutbound {
    async fn send_text(&self, _: &serde_json::Value, _: &str, _: &str) -> BaseResult<String> {
        Ok("null_msg_id".to_string())
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> BaseResult<()> {
        Ok(())
    }
    async fn edit_message(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> BaseResult<()> {
        Ok(())
    }
    async fn add_reaction(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> BaseResult<()> {
        Ok(())
    }
}

// ── Registry tests ──

#[test]
fn register_and_list() {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestPlugin::conversational("telegram")));
    registry.register(Arc::new(TestPlugin::event_driven("github")));

    let types = registry.list();
    assert_eq!(types.len(), 2);
    assert!(types.contains(&"telegram"));
    assert!(types.contains(&"github"));
}

#[test]
fn get_registered_plugin() -> Result<()> {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestPlugin::conversational("feishu")));

    let entry = registry.get("feishu").context("expected feishu entry")?;
    assert_eq!(entry.plugin.channel_type(), "feishu");
    assert_eq!(
        entry.plugin.capabilities().channel_kind,
        ChannelKind::Conversational
    );
    Ok(())
}

#[test]
fn get_unknown_returns_none() {
    let registry = ChannelRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn register_overwrites_same_type() -> Result<()> {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestPlugin::conversational("slack")));
    registry.register(Arc::new(TestPlugin::event_driven("slack")));

    let types = registry.list();
    assert_eq!(types.len(), 1);
    let entry = registry.get("slack").context("expected slack entry")?;
    assert_eq!(
        entry.plugin.capabilities().channel_kind,
        ChannelKind::EventDriven
    );
    Ok(())
}

#[test]
fn list_is_sorted() {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestPlugin::conversational("telegram")));
    registry.register(Arc::new(TestPlugin::conversational("feishu")));
    registry.register(Arc::new(TestPlugin::event_driven("github")));

    let types = registry.list();
    assert_eq!(types, vec!["feishu", "github", "telegram"]);
}

#[test]
fn empty_registry() {
    let registry = ChannelRegistry::new();
    assert!(registry.list().is_empty());
    assert!(registry.get("anything").is_none());
}

#[tokio::test]
async fn outbound_delegates_through_plugin() -> Result<()> {
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestPlugin::conversational("test")));

    let entry = registry.get("test").context("expected test entry")?;
    let outbound = entry.plugin.outbound();
    let msg_id = outbound
        .send_text(&serde_json::json!({}), "chat_1", "hello")
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(msg_id, "null_msg_id");
    Ok(())
}
