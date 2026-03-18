use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::SinkExt;
use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::capabilities::ChannelCapabilities;
use crate::kernel::channel::capabilities::ChannelKind;
use crate::kernel::channel::capabilities::InboundMode;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::message::InboundMessage;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::channel::plugin::ChannelPlugin;
use crate::kernel::channel::plugin::InboundEventSender;
use crate::kernel::channel::plugin::InboundKind;
use crate::kernel::channel::plugin::ReceiverFactory;

pub const FEISHU_CHANNEL_TYPE: &str = "feishu";
const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
const FEISHU_DOMAIN: &str = "https://open.feishu.cn";
const FEISHU_MAX_MESSAGE_LEN: usize = 30_000;

const DEFAULT_PING_INTERVAL_SECS: u64 = 120;
const RECONNECT_DELAY_SECS: u64 = 5;

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

// ── Plugin ──

pub struct FeishuChannel {
    client: reqwest::Client,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for FeishuChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelPlugin for FeishuChannel {
    fn channel_type(&self) -> &str {
        FEISHU_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::WebSocket,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let c: FeishuConfig = serde_json::from_value(config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        if c.app_id.is_empty() || c.app_secret.is_empty() {
            return Err(ErrorCode::config(
                "feishu app_id and app_secret are required",
            ));
        }
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FeishuOutbound {
            client: self.client.clone(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(FeishuReceiverFactory {
            client: self.client.clone(),
        }))
    }
}

// ── ReceiverFactory ──

struct FeishuReceiverFactory {
    client: reqwest::Client,
}

#[async_trait]
impl ReceiverFactory for FeishuReceiverFactory {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let config: FeishuConfig = serde_json::from_value(account.config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        let client = self.client.clone();
        let account_id = account.channel_account_id.clone();

        let handle = tokio::spawn(async move {
            tracing::info!(account_id = %account_id, "feishu WebSocket receiver started");
            let mut backoff_secs = RECONNECT_DELAY_SECS;
            const MAX_BACKOFF_SECS: u64 = 120;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!(account_id = %account_id, "feishu receiver cancelled");
                        return;
                    }
                    result = ws_receive_loop(&client, &config, &event_tx) => {
                        match result {
                            Ok(()) => {
                                tracing::info!(account_id = %account_id, "feishu WebSocket closed, reconnecting");
                                backoff_secs = RECONNECT_DELAY_SECS;
                            }
                            Err(e) => {
                                tracing::error!(account_id = %account_id, backoff_secs, error = %e, "feishu WebSocket error, reconnecting");
                            }
                        }
                    }
                }
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                }
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
            }
        });

        Ok(handle)
    }
}

// ── Outbound ──

struct FeishuOutbound {
    client: reqwest::Client,
}

impl FeishuOutbound {
    fn extract_credentials(config: &serde_json::Value) -> Result<(String, String)> {
        let app_id = config
            .get("app_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_id"))?;
        let app_secret = config
            .get("app_secret")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_secret"))?;
        Ok((app_id.to_string(), app_secret.to_string()))
    }
}

#[async_trait]
impl ChannelOutbound for FeishuOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let (app_id, app_secret) = Self::extract_credentials(config)?;
        let token = get_tenant_token(&self.client, &app_id, &app_secret).await?;

        let url = format!("{FEISHU_API}/im/v1/messages?receive_id_type=chat_id");
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content,
        });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu send: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu send response: {e}")))?;
        let msg_id = json["data"]["message_id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(msg_id)
    }

    async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _text: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "feishu channel does not support edit_message",
        ))
    }

    async fn add_reaction(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "feishu channel does not support reactions",
        ))
    }
}

// ── WebSocket long connection ──

async fn get_tenant_token(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let url = format!("{FEISHU_API}/auth/v3/tenant_access_token/internal");
    let body = serde_json::json!({
        "app_id": app_id,
        "app_secret": app_secret,
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth: {e}")))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown");
        return Err(ErrorCode::internal(format!(
            "feishu auth failed: code={code}, msg={msg}"
        )));
    }

    json["tenant_access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ErrorCode::internal(format!(
                "feishu: missing tenant_access_token in response: {json}"
            ))
        })
}

async fn get_ws_endpoint(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64)> {
    let url = format!("{FEISHU_DOMAIN}/callback/ws/endpoint");
    let resp = client
        .post(&url)
        .header("locale", "zh")
        .json(&serde_json::json!({
            "AppID": app_id,
            "AppSecret": app_secret,
        }))
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint HTTP {status}: {body}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown error");
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint failed: code={code}, msg={msg}"
        )));
    }

    let ws_url = json["data"]["URL"]
        .as_str()
        .ok_or_else(|| ErrorCode::internal("feishu ws endpoint: missing URL"))?
        .to_string();

    let ping_interval = json["data"]["ClientConfig"]["PingInterval"]
        .as_u64()
        .unwrap_or(DEFAULT_PING_INTERVAL_SECS);

    Ok((ws_url, ping_interval))
}

async fn ws_receive_loop(
    client: &reqwest::Client,
    config: &FeishuConfig,
    event_tx: &InboundEventSender,
) -> Result<()> {
    let (ws_url, ping_interval_secs) =
        get_ws_endpoint(client, &config.app_id, &config.app_secret).await?;

    tracing::info!(url = %ws_url, ping_interval = ping_interval_secs, "feishu WebSocket connecting");

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws connect: {e}")))?;

    let (mut write, mut read) = ws_stream.split();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(ping_interval_secs));
    ping_interval.tick().await;

    tracing::info!("feishu WebSocket connected");

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_ws_message(&text, config, event_tx);
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("feishu WebSocket closed by server");
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(ErrorCode::internal(format!("feishu ws read: {e}")));
                    }
                    _ => {}
                }
            }
            _ = ping_interval.tick() => {
                if write.send(Message::Ping(vec![])).await.is_err() {
                    return Err(ErrorCode::internal("feishu ws ping failed"));
                }
            }
        }
    }
}

fn handle_ws_message(text: &str, config: &FeishuConfig, event_tx: &InboundEventSender) {
    let json: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "feishu ws: invalid JSON frame");
            return;
        }
    };

    let msg_type = json
        .pointer("/header/type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if msg_type == "pong" {
        return;
    }

    let event_type = json
        .pointer("/header/event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type == "im.message.receive_v1" {
        if let Some(event_data) = json.get("event") {
            let sender_id = event_data
                .pointer("/sender/sender_id/open_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !is_allowed(&config.allow_from, sender_id) {
                tracing::warn!(sender_id, "feishu: sender not in allow_from, denied");
                return;
            }

            if let Some(inbound) = parse_feishu_message(event_data) {
                if event_tx.try_send(inbound).is_err() {
                    tracing::warn!("feishu ws: event channel full or receiver dropped");
                }
            }
        }
    }
}

fn is_allowed(allow_from: &[String], sender_id: &str) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    if allow_from.iter().any(|s| s == "*") {
        return true;
    }
    allow_from.iter().any(|s| s == sender_id)
}

fn parse_feishu_message(event: &serde_json::Value) -> Option<InboundEvent> {
    let message = event.get("message")?;
    let sender = event.get("sender")?.get("sender_id")?;

    let msg_id = message.get("message_id")?.as_str()?;
    let chat_id = message.get("chat_id")?.as_str()?;
    let sender_id = sender.get("open_id")?.as_str()?;
    let msg_type = message.get("message_type")?.as_str()?;

    if msg_type != "text" {
        return None;
    }

    let content_str = message.get("content")?.as_str()?;
    let content: serde_json::Value = serde_json::from_str(content_str).ok()?;
    let text = content.get("text")?.as_str()?;

    let timestamp = message
        .get("create_time")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ms| ms / 1000)
        .unwrap_or(0);

    Some(InboundEvent::Message(InboundMessage {
        message_id: msg_id.to_string(),
        chat_id: chat_id.to_string(),
        sender_id: sender_id.to_string(),
        sender_name: String::new(),
        text: text.to_string(),
        attachments: vec![],
        timestamp,
    }))
}
