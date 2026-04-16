use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::config::FeishuChannelConfig;
use super::delivery::FeishuMessageSink;
use super::token::TokenCache;
use crate::agent::run_manager::ConversationKey;
use crate::agent::run_manager::RunManager;
use crate::agent::run_manager::SendOutcome;
use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::ToolMode;
use crate::error::Result;
use crate::gateway::delivery::stream as stream_delivery;
use crate::gateway::delivery::stream::StreamDeliveryConfig;
use crate::gateway::Channel;

pub struct FeishuChannel {
    config: FeishuChannelConfig,
    run_manager: Arc<RunManager>,
    client: reqwest::Client,
    token_cache: TokenCache,
    bot_open_id: tokio::sync::OnceCell<String>,
}

impl FeishuChannel {
    pub fn new(config: FeishuChannelConfig, run_manager: Arc<RunManager>) -> Self {
        Self {
            config,
            run_manager,
            client: reqwest::Client::new(),
            token_cache: TokenCache::new(),
            bot_open_id: tokio::sync::OnceCell::new(),
        }
    }

    pub fn spawn(
        conf: FeishuChannelConfig,
        run_manager: Arc<RunManager>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let agent = run_manager.agent().clone();
        let ch = Arc::new(Self::new(conf, run_manager));
        tokio::spawn(async move {
            if let Err(e) = ch.run(agent, cancel).await {
                tracing::error!(channel = "feishu", error = %e, "channel exited");
            }
        })
    }

    async fn handle_message(self: &Arc<Self>, msg: super::message::ParsedMessage) {
        let key = ConversationKey::new("feishu", &format!("{}:{}", msg.chat_id, msg.sender_id));
        tracing::info!(
            channel = "feishu",
            chat_id = %msg.chat_id,
            sender_id = %msg.sender_id,
            "received message"
        );

        // Spawn so we don't block the websocket receive loop
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut input: Vec<evot_engine::Content> = Vec::new();

            if let Some(ref pid) = msg.parent_id {
                match super::delivery::fetch_message_content(
                    &this.client,
                    &this.token_cache,
                    &this.config.app_id,
                    &this.config.app_secret,
                    pid,
                )
                .await
                {
                    Ok(Some(parent)) => {
                        if let Some(quoted) = parent.text {
                            input.push(evot_engine::Content::Text {
                                text: format!("[Quoted message: {quoted}]"),
                            });
                        }
                        input.extend(
                            super::delivery::resolve_message_parts(
                                &this.client,
                                &this.token_cache,
                                &this.config.app_id,
                                &this.config.app_secret,
                                &parent.message_id,
                                &parent.parts,
                            )
                            .await,
                        );
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(
                            channel = "feishu",
                            parent_id = %pid,
                            error = %e,
                            "failed to fetch quoted message"
                        );
                    }
                }
            }

            input.extend(
                super::delivery::resolve_message_parts(
                    &this.client,
                    &this.token_cache,
                    &this.config.app_id,
                    &this.config.app_secret,
                    &msg.message_id,
                    &msg.parts,
                )
                .await,
            );

            if input.is_empty() {
                return;
            }

            let request = QueryRequest::with_input(input).mode(ToolMode::Headless);
            match this.run_manager.send(&key, request).await {
                Ok(SendOutcome::Started(mut run)) => {
                    let sink = FeishuMessageSink::new(
                        this.client.clone(),
                        this.token_cache.clone(),
                        this.config.app_id.clone(),
                        this.config.app_secret.clone(),
                    );
                    let config = StreamDeliveryConfig::default();
                    if let Err(e) =
                        stream_delivery::deliver(&sink, &msg.chat_id, &mut run, &config).await
                    {
                        tracing::error!(channel = "feishu", error = %e, "delivery failed");
                    }
                }
                Ok(SendOutcome::Steered) => {
                    // Message routed to active run — nothing to deliver
                }
                Err(e) => {
                    tracing::error!(channel = "feishu", error = %e, "agent query failed");
                    let sink = FeishuMessageSink::new(
                        this.client.clone(),
                        this.token_cache.clone(),
                        this.config.app_id.clone(),
                        this.config.app_secret.clone(),
                    );
                    let _ = crate::gateway::delivery::MessageSink::send_text(
                        &sink,
                        &msg.chat_id,
                        &format!("Error: {e}"),
                    )
                    .await;
                }
            }
        });
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    // NOTE: `_agent` is unused — FeishuChannel holds a RunManager (injected at
    // construction) which owns the Agent reference. The parameter is kept to
    // satisfy the generic Channel trait; this is a known trait-reuse trade-off.
    async fn run(self: Arc<Self>, _agent: Arc<Agent>, cancel: CancellationToken) -> Result<()> {
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
                async move {
                    self_inner.handle_message(msg).await;
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
