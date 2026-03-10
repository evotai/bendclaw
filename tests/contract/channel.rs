//! Contract tests — shared suite that every ChannelPlugin implementation must pass.

use std::sync::Arc;

use bendclaw::kernel::channel::account::ChannelAccount;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::InboundKind;

fn test_account(channel_type: &str) -> ChannelAccount {
    let config = match channel_type {
        "telegram" => serde_json::json!({"token": "test-token-123"}),
        "feishu" => serde_json::json!({"app_id": "cli_test", "app_secret": "secret_test"}),
        "github" => serde_json::json!({"token": "ghp_test123"}),
        _ => serde_json::json!({}),
    };
    ChannelAccount {
        channel_account_id: "ca_test".into(),
        channel_type: channel_type.into(),
        external_account_id: "acc_test".into(),
        agent_id: "agent_test".into(),
        user_id: "user_test".into(),
        config,
        enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

async fn run_contract(plugin: Arc<dyn ChannelPlugin>) {
    contract_channel_type_not_empty(plugin.as_ref());
    contract_capabilities_consistent(plugin.as_ref());
    contract_outbound_available(plugin.as_ref());
    contract_inbound_kind_consistent(plugin.as_ref());
    let _ = test_account(plugin.channel_type()); // ensure test_account builds
}

fn contract_channel_type_not_empty(plugin: &dyn ChannelPlugin) {
    assert!(
        !plugin.channel_type().is_empty(),
        "channel_type() must not be empty"
    );
}

fn contract_capabilities_consistent(plugin: &dyn ChannelPlugin) {
    let caps = plugin.capabilities();
    assert!(caps.max_message_len > 0, "max_message_len must be positive");
    let caps2 = plugin.capabilities();
    assert_eq!(caps.channel_kind, caps2.channel_kind);
    assert_eq!(caps.inbound_mode, caps2.inbound_mode);
    assert_eq!(caps.max_message_len, caps2.max_message_len);
}

fn contract_outbound_available(plugin: &dyn ChannelPlugin) {
    let _outbound = plugin.outbound();
}

fn contract_inbound_kind_consistent(plugin: &dyn ChannelPlugin) {
    use bendclaw::kernel::channel::InboundMode;
    let caps = plugin.capabilities();
    match plugin.inbound() {
        InboundKind::Webhook(_) => {
            assert_eq!(
                caps.inbound_mode,
                InboundMode::Webhook,
                "plugin '{}' returns Webhook inbound but capabilities say {:?}",
                plugin.channel_type(),
                caps.inbound_mode
            );
        }
        InboundKind::Receiver(_) => {
            assert!(
                caps.inbound_mode == InboundMode::WebSocket
                    || caps.inbound_mode == InboundMode::Polling,
                "plugin '{}' returns Receiver inbound but capabilities say {:?}",
                plugin.channel_type(),
                caps.inbound_mode
            );
        }
        InboundKind::None => {}
    }
}

// ── Contract test for each plugin ──

use bendclaw::kernel::channel::plugins::feishu::FeishuChannel;
use bendclaw::kernel::channel::plugins::github::GitHubChannel;
use bendclaw::kernel::channel::plugins::http_api::HttpApiChannel;
use bendclaw::kernel::channel::plugins::telegram::TelegramChannel;

#[tokio::test]
async fn http_api_passes_contract() {
    run_contract(Arc::new(HttpApiChannel::new())).await;
}

#[tokio::test]
async fn telegram_passes_contract() {
    run_contract(Arc::new(TelegramChannel::new())).await;
}

#[tokio::test]
async fn feishu_passes_contract() {
    run_contract(Arc::new(FeishuChannel::new())).await;
}

#[tokio::test]
async fn github_passes_contract() {
    run_contract(Arc::new(GitHubChannel::new())).await;
}
