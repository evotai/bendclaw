use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
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

pub const TELEGRAM_CHANNEL_TYPE: &str = "telegram";
const TELEGRAM_API: &str = "https://api.telegram.org";
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

const POLL_TIMEOUT_SECS: u64 = 30;
const RETRY_DELAY_SECS: u64 = 5;

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub token: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

// ── Plugin ──

pub struct TelegramChannel {
    client: reqwest::Client,
}

impl TelegramChannel {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for TelegramChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelPlugin for TelegramChannel {
    fn channel_type(&self) -> &str {
        TELEGRAM_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::Polling,
            supports_edit: true,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: true,
            max_message_len: TELEGRAM_MAX_MESSAGE_LEN,
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let c: TelegramConfig = serde_json::from_value(config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid telegram config: {e}")))?;
        if c.token.is_empty() {
            return Err(ErrorCode::config("telegram token is required"));
        }
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(TelegramOutbound {
            client: self.client.clone(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(TelegramReceiverFactory {
            client: self.client.clone(),
        }))
    }
}

// ── ReceiverFactory ──

struct TelegramReceiverFactory {
    client: reqwest::Client,
}

#[async_trait]
impl ReceiverFactory for TelegramReceiverFactory {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let config: TelegramConfig = serde_json::from_value(account.config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid telegram config: {e}")))?;
        let client = self.client.clone();
        let account_id = account.channel_account_id.clone();

        let handle = tokio::spawn(async move {
            tracing::info!(account_id = %account_id, "telegram long-poll receiver started");
            let mut offset: i64 = 0;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!(account_id = %account_id, "telegram receiver cancelled");
                        return;
                    }
                    result = poll_updates(&client, &config, &mut offset, &event_tx) => {
                        if let Err(e) = result {
                            tracing::error!(account_id = %account_id, error = %e, "telegram poll error, retrying");
                            tokio::select! {
                                _ = cancel.cancelled() => return,
                                _ = tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)) => {}
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }
}

// ── Outbound ──

struct TelegramOutbound {
    client: reqwest::Client,
}

impl TelegramOutbound {
    fn api_url(token: &str, method: &str) -> String {
        format!("{TELEGRAM_API}/bot{token}/{method}")
    }

    fn extract_token(config: &serde_json::Value) -> Result<String> {
        config
            .get("token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| ErrorCode::config("telegram config missing token"))
    }
}

#[async_trait]
impl ChannelOutbound for TelegramOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&token, "sendMessage");
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("telegram sendMessage: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("telegram response: {e}")))?;
        let msg_id = json["result"]["message_id"]
            .as_i64()
            .ok_or_else(|| {
                ErrorCode::internal(format!(
                    "telegram sendMessage: missing message_id in response: {json}"
                ))
            })?
            .to_string();
        Ok(msg_id)
    }

    async fn send_typing(&self, config: &serde_json::Value, chat_id: &str) -> Result<()> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&token, "sendChatAction");
        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });
        let _ = self.client.post(&url).json(&body).send().await;
        Ok(())
    }

    async fn edit_message(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&token, "editMessageText");
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": msg_id.parse::<i64>().map_err(|e| {
                ErrorCode::internal(format!("telegram editMessageText: invalid message_id '{msg_id}': {e}"))
            })?,
            "text": text,
            "parse_mode": "Markdown",
        });
        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("telegram editMessageText: {e}")))?;
        Ok(())
    }

    async fn add_reaction(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&token, "setMessageReaction");
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": msg_id.parse::<i64>().map_err(|e| {
                ErrorCode::internal(format!("telegram setMessageReaction: invalid message_id '{msg_id}': {e}"))
            })?,
            "reaction": [{"type": "emoji", "emoji": emoji}],
        });
        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("telegram setMessageReaction: {e}")))?;
        tracing::info!(
            channel_type = "telegram",
            chat_id,
            message_id = msg_id,
            emoji,
            "reaction sent"
        );
        Ok(())
    }

    async fn update_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        self.edit_message(config, chat_id, msg_id, text).await
    }
}

// ── Long polling ──

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    date: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TelegramUser {
    id: i64,
    first_name: String,
    #[serde(default)]
    last_name: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

impl TelegramUser {
    fn display_name(&self) -> String {
        match &self.last_name {
            Some(last) => format!("{} {last}", self.first_name),
            None => self.first_name.clone(),
        }
    }
}

fn is_allowed(allow_from: &[String], user: &TelegramUser) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    let user_id_str = user.id.to_string();
    let username = user.username.as_deref().unwrap_or("");
    for entry in allow_from {
        let id_part = entry.split('|').next().unwrap_or(entry.as_str());
        if id_part == user_id_str {
            return true;
        }
        if let Some(uname_part) = entry.split('|').nth(1) {
            if !uname_part.is_empty() && uname_part == username {
                return true;
            }
        }
    }
    false
}

async fn poll_updates(
    client: &reqwest::Client,
    config: &TelegramConfig,
    offset: &mut i64,
    event_tx: &InboundEventSender,
) -> Result<()> {
    let url = format!("{TELEGRAM_API}/bot{}/getUpdates", config.token);
    let body = serde_json::json!({
        "offset": *offset,
        "timeout": POLL_TIMEOUT_SECS,
        "allowed_updates": ["message"],
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(POLL_TIMEOUT_SECS + 10))
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("telegram getUpdates: {e}")))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("telegram getUpdates response: {e}")))?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let desc = json["description"].as_str().unwrap_or("unknown error");
        return Err(ErrorCode::internal(format!(
            "telegram getUpdates failed: {desc}"
        )));
    }

    let updates: Vec<TelegramUpdate> =
        serde_json::from_value(json["result"].clone()).map_err(|e| {
            ErrorCode::internal(format!("telegram getUpdates: failed to parse result: {e}"))
        })?;

    for update in updates {
        *offset = update.update_id + 1;

        if let Some(msg) = update.message {
            let text = match msg.text {
                Some(ref t) if !t.is_empty() => t.clone(),
                _ => continue,
            };
            let user = msg.from.unwrap_or(TelegramUser {
                id: 0,
                first_name: "unknown".into(),
                last_name: None,
                username: None,
            });
            if !is_allowed(&config.allow_from, &user) {
                tracing::warn!(
                    sender_id = user.id,
                    "telegram: sender not in allow_from, denied"
                );
                continue;
            }

            // Add thumbsup reaction to acknowledge the message.
            let _ = send_reaction(
                client,
                &config.token,
                msg.chat.id,
                msg.message_id,
                "\u{1F44D}",
            )
            .await;

            let event = InboundEvent::Message(InboundMessage {
                message_id: msg.message_id.to_string(),
                chat_id: msg.chat.id.to_string(),
                sender_id: user.id.to_string(),
                sender_name: user.display_name(),
                text,
                attachments: vec![],
                timestamp: msg.date,
            });
            if let crate::kernel::channel::delivery::backpressure::BackpressureResult::Rejected =
                event_tx.send(event)
            {
                tracing::warn!("telegram: event receiver dropped");
            }
        }
    }

    Ok(())
}

async fn send_reaction(
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    message_id: i64,
    emoji: &str,
) -> Result<()> {
    let url = format!("{TELEGRAM_API}/bot{token}/setMessageReaction");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": [{"type": "emoji", "emoji": emoji}],
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("telegram setMessageReaction: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(status = %status, body, "telegram setMessageReaction failed");
    }
    Ok(())
}
