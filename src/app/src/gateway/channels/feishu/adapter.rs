use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::config::FeishuChannelConfig;
use super::delivery::FeishuMessageSink;
use super::token::TokenCache;
use crate::agent::run_manager::RunManager;
use crate::agent::run_manager::SendOutcome;
use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::SessionLocator;
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

    async fn handle_message(
        self: &Arc<Self>,
        msg: super::message::ParsedMessage,
        bot_open_id: &str,
    ) {
        // Build locator: topic messages get their own session scope,
        // non-topic messages share a session per chat+user.
        let scope = if let Some(ref tid) = msg.thread_id {
            format!("chat:{}:topic:{}", msg.chat_id, tid)
        } else if let Some(ref rid) = msg.root_id {
            format!("chat:{}:topic:{}", msg.chat_id, rid)
        } else if let Some(ref pid) = msg.parent_id {
            format!("chat:{}:topic:{}", msg.chat_id, pid)
        } else {
            format!("chat:{}:user:{}", msg.chat_id, msg.sender_id)
        };
        let locator = SessionLocator::new("feishu", &scope);
        tracing::info!(
            channel = "feishu",
            chat_id = %msg.chat_id,
            sender_id = %msg.sender_id,
            message_id = %msg.message_id,
            thread_id = ?msg.thread_id,
            root_id = ?msg.root_id,
            parent_id = ?msg.parent_id,
            scope = %scope,
            session_id = %locator.session_id(),
            "received message"
        );

        // Spawn so we don't block the websocket receive loop
        let this = Arc::clone(self);
        let bot_open_id = bot_open_id.to_string();
        tokio::spawn(async move {
            let mut input: Vec<evot_engine::Content> = Vec::new();

            // Determine if this message is in a topic (thread)
            let thread_id = msg.thread_id.as_deref();
            let parent_id = msg.parent_id.as_deref();

            if thread_id.is_some() || msg.root_id.is_some() || parent_id.is_some() {
                // ── Topic context: root message + thread replies ──

                // Fetch root message content if we have parent_id
                if let Some(pid) = parent_id {
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
                                    text: format!("[Topic root]: {quoted}"),
                                });
                            }
                            // Only download images from root (text already added above)
                            for part in &parent.parts {
                                if let super::message::MessagePart::ImageKey(image_key) = part {
                                    match super::delivery::download_image(
                                        &this.client,
                                        &this.token_cache,
                                        &this.config.app_id,
                                        &this.config.app_secret,
                                        &parent.message_id,
                                        image_key,
                                    )
                                    .await
                                    {
                                        Ok(img) => {
                                            input.push(evot_engine::Content::Image {
                                                data: img.data_base64,
                                                mime_type: img.mime_type,
                                            });
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                channel = "feishu",
                                                image_key,
                                                error = %e,
                                                "failed to download root image"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(
                                channel = "feishu",
                                parent_id = pid,
                                error = %e,
                                "failed to fetch topic root message"
                            );
                        }
                    }
                }

                // Fetch thread replies.
                // Primary: use thread_id (omt_xxx) with container_id_type=thread.
                // Fallback: if no thread_id, use chat history filtered by root_id.
                let topic_replies = if let Some(tid) = thread_id {
                    super::delivery::fetch_thread_messages(
                        &this.client,
                        &this.token_cache,
                        &this.config.app_id,
                        &this.config.app_secret,
                        tid,
                    )
                    .await
                } else {
                    // Fallback: fetch chat history and filter by root_id/parent_id
                    let topic_root = msg.root_id.as_deref().or(parent_id);
                    match topic_root {
                        Some(rid) => super::delivery::fetch_chat_history(
                            &this.client,
                            &this.token_cache,
                            &this.config.app_id,
                            &this.config.app_secret,
                            &msg.chat_id,
                            msg.create_time,
                            50,
                        )
                        .await
                        .map(|msgs| {
                            msgs.into_iter()
                                .filter(|m| {
                                    m.root_id.as_deref() == Some(rid) || m.message_id == rid
                                })
                                .collect()
                        }),
                        None => Ok(Vec::new()),
                    }
                };

                match topic_replies {
                    Ok(replies) => {
                        // Filter: exclude current message and messages after it
                        let mut filtered: Vec<_> = replies
                            .into_iter()
                            .filter(|r| {
                                r.message_id != msg.message_id
                                    && (msg.create_time == 0 || r.create_time <= msg.create_time)
                            })
                            .collect();

                        // Keep only the most recent 50
                        if filtered.len() > 50 {
                            filtered = filtered.split_off(filtered.len() - 50);
                        }

                        // Count images across replies; limit to 10 (prefer most recent)
                        let total_images: usize = filtered
                            .iter()
                            .flat_map(|r| &r.parts)
                            .filter(|p| matches!(p, super::message::MessagePart::ImageKey(_)))
                            .count();
                        let mut images_to_skip = total_images.saturating_sub(10);

                        for reply in &filtered {
                            if let Some(ref text) = reply.text {
                                input.push(evot_engine::Content::Text {
                                    text: format!("[Topic reply]: {text}"),
                                });
                            }
                            // Resolve parts, skipping oldest images beyond limit
                            for part in &reply.parts {
                                match part {
                                    super::message::MessagePart::Text(_) => {
                                        // Text already added via reply.text label above
                                    }
                                    super::message::MessagePart::ImageKey(image_key) => {
                                        if images_to_skip > 0 {
                                            images_to_skip -= 1;
                                            continue;
                                        }
                                        match super::delivery::download_image(
                                            &this.client,
                                            &this.token_cache,
                                            &this.config.app_id,
                                            &this.config.app_secret,
                                            &reply.message_id,
                                            image_key,
                                        )
                                        .await
                                        {
                                            Ok(img) => {
                                                input.push(evot_engine::Content::Image {
                                                    data: img.data_base64,
                                                    mime_type: img.mime_type,
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    channel = "feishu",
                                                    image_key,
                                                    error = %e,
                                                    "failed to download thread image"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            channel = "feishu",
                            error = %e,
                            "failed to fetch thread replies"
                        );
                    }
                }
            } else if msg.chat_type == "group" {
                // ── Chat history context: recent messages before current ──
                match super::delivery::fetch_chat_history(
                    &this.client,
                    &this.token_cache,
                    &this.config.app_id,
                    &this.config.app_secret,
                    &msg.chat_id,
                    msg.create_time,
                    30,
                )
                .await
                {
                    Ok(history) => {
                        let filtered: Vec<_> = history
                            .into_iter()
                            .filter(|m| {
                                m.message_id != msg.message_id
                                    && m.sender_id.as_deref() != Some(&bot_open_id)
                            })
                            .collect();

                        if !filtered.is_empty() {
                            input.push(evot_engine::Content::Text {
                                text: "[Chat history]".to_string(),
                            });
                            for m in &filtered {
                                if let Some(ref text) = m.text {
                                    input.push(evot_engine::Content::Text {
                                        text: format!("[Chat message]: {text}"),
                                    });
                                }
                                // No image download for chat history — text only
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            channel = "feishu",
                            chat_id = %msg.chat_id,
                            error = %e,
                            "failed to fetch chat history, degrading to current message only"
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
            match this.run_manager.send(&locator, request).await {
                Ok(SendOutcome::Started(mut run)) => {
                    let sink = FeishuMessageSink::new(
                        this.client.clone(),
                        this.token_cache.clone(),
                        this.config.app_id.clone(),
                        this.config.app_secret.clone(),
                    );
                    let sink = if msg.chat_type == "group" {
                        sink.with_reply_to(msg.message_id.clone())
                    } else {
                        sink
                    };
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
                    let sink = if msg.chat_type == "group" {
                        sink.with_reply_to(msg.message_id.clone())
                    } else {
                        sink
                    };
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
                let bot_id = bot_open_id.to_string();
                async move {
                    self_inner.handle_message(msg, &bot_id).await;
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
