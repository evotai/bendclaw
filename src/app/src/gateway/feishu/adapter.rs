use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::config::FeishuChannelConfig;
use super::config::FEISHU_MAX_MESSAGE_LEN;
use super::token::TokenCache;
use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::RunEventPayload;
use crate::error::Result;
use crate::gateway::Channel;

pub struct FeishuChannel {
    config: FeishuChannelConfig,
    session_map: SessionMap,
    client: reqwest::Client,
    token_cache: TokenCache,
    bot_open_id: tokio::sync::OnceCell<String>,
}

impl FeishuChannel {
    pub fn new(config: FeishuChannelConfig) -> Self {
        Self {
            config,
            session_map: SessionMap::new(),
            client: reqwest::Client::new(),
            token_cache: TokenCache::new(),
            bot_open_id: tokio::sync::OnceCell::new(),
        }
    }

    pub fn spawn(
        conf: FeishuChannelConfig,
        agent: Arc<Agent>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let ch = Arc::new(Self::new(conf));
        tokio::spawn(async move {
            if let Err(e) = ch.run(agent, cancel).await {
                tracing::error!(channel = "feishu", error = %e, "channel exited");
            }
        })
    }

    async fn handle_message(&self, agent: &Agent, msg: super::message::ParsedMessage) {
        let session_key = format!("{}:{}", msg.chat_id, msg.sender_id);
        let session_id = self.session_map.get(&session_key).await;
        tracing::info!(
            channel = "feishu",
            chat_id = %msg.chat_id,
            sender_id = %msg.sender_id,
            session_id = ?session_id,
            "received message"
        );

        let request = QueryRequest::text(&msg.text).session_id(session_id);
        match agent.query(request).await {
            Ok(mut stream) => {
                // Store the real session_id created by the agent
                self.session_map
                    .set(&session_key, stream.session_id.clone())
                    .await;

                let reply = collect_reply(&mut stream).await;
                if !reply.is_empty() {
                    self.send_reply(&msg.chat_id, &reply).await;
                }
            }
            Err(e) => {
                tracing::error!(channel = "feishu", error = %e, "agent query failed");
                self.send_reply(&msg.chat_id, &format!("Error: {e}")).await;
            }
        }
    }

    async fn send_reply(&self, chat_id: &str, reply: &str) {
        // Truncate if too long (char-boundary safe)
        let reply = if reply.len() > FEISHU_MAX_MESSAGE_LEN {
            let boundary = reply
                .char_indices()
                .take_while(|(i, _)| *i <= FEISHU_MAX_MESSAGE_LEN)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut truncated = reply[..boundary].to_string();
            truncated.push_str("\n\n... (truncated)");
            truncated
        } else {
            reply.to_string()
        };

        if let Err(e) = super::outbound::send_text(
            &self.client,
            &self.token_cache,
            &self.config.app_id,
            &self.config.app_secret,
            chat_id,
            &reply,
        )
        .await
        {
            tracing::error!(channel = "feishu", chat_id, error = %e, "send failed");
        }
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn run(self: Arc<Self>, agent: Arc<Agent>, cancel: CancellationToken) -> Result<()> {
        tracing::info!(channel = "feishu", "channel started");

        let bot_open_id = self
            .bot_open_id
            .get_or_try_init(|| async {
                super::token::fetch_bot_open_id(
                    &self.client,
                    &self.config.app_id,
                    &self.config.app_secret,
                    &self.token_cache,
                )
                .await
            })
            .await?;
        tracing::info!(channel = "feishu", bot_open_id, "resolved bot identity");

        let mut attempt: u32 = 0;
        loop {
            if cancel.is_cancelled() {
                break;
            }

            let self_ref = self.clone();
            let agent_ref = agent.clone();
            let ctx = super::ws::WsContext {
                client: &self.client,
                app_id: &self.config.app_id,
                app_secret: &self.config.app_secret,
                token_cache: &self.token_cache,
                config: &self.config,
                bot_open_id,
            };
            let result = super::ws::ws_receive_loop(&ctx, &cancel, |msg| {
                let self_inner = self_ref.clone();
                let agent_inner = agent_ref.clone();
                async move {
                    self_inner.handle_message(&agent_inner, msg).await;
                }
            })
            .await;

            if cancel.is_cancelled() {
                break;
            }

            match result {
                Ok(()) => {
                    tracing::info!(channel = "feishu", "websocket closed cleanly, reconnecting");
                    attempt = 0;
                }
                Err(e) => {
                    tracing::warn!(channel = "feishu", error = %e, attempt, "websocket error");
                    attempt = attempt.saturating_add(1);
                }
            }

            // Exponential backoff: 1s, 2s, 4s, 8s, ... max 60s
            let backoff = Duration::from_secs(
                1u64.saturating_mul(2u64.saturating_pow(attempt.min(6)))
                    .min(60),
            );
            tracing::info!(
                channel = "feishu",
                backoff_secs = backoff.as_secs(),
                "reconnecting"
            );

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(backoff) => {}
            }
        }

        tracing::info!(channel = "feishu", "channel stopped");
        Ok(())
    }
}

// ── Collect final reply text from QueryStream ──

async fn collect_reply(stream: &mut crate::agent::QueryStream) -> String {
    let mut parts = Vec::new();
    while let Some(event) = stream.next().await {
        if let RunEventPayload::AssistantDelta {
            delta: Some(delta), ..
        } = &event.payload
        {
            if !delta.is_empty() {
                parts.push(delta.clone());
            }
        }
    }
    parts.join("")
}

// ── channel-private session state ──

struct SessionMap {
    inner: Mutex<HashMap<String, String>>,
}

impl SessionMap {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    async fn get(&self, key: &str) -> Option<String> {
        self.inner.lock().await.get(key).cloned()
    }

    async fn set(&self, key: &str, session_id: String) {
        self.inner.lock().await.insert(key.to_string(), session_id);
    }
}
