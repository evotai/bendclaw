use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use crate::channels::model::message::InboundEvent;
use crate::channels::routing::debouncer::debounce;
use crate::channels::routing::debouncer::ChatJob;
use crate::channels::routing::debouncer::DebounceConfig;
use crate::channels::routing::debouncer::DebounceResult;
use crate::channels::routing::debouncer::DebouncedInput;
pub type ChatHandler =
    Arc<dyn Fn(DebouncedInput) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct ChatRouterConfig {
    pub per_chat_capacity: usize,
    pub idle_timeout: Duration,
}

impl Default for ChatRouterConfig {
    fn default() -> Self {
        Self {
            per_chat_capacity: 32,
            idle_timeout: Duration::from_secs(60),
        }
    }
}

/// Routes inbound events to per-chat_id serial queues.
/// Same chat_id → sequential processing. Different chat_ids → concurrent.
pub struct ChatRouter {
    config: ChatRouterConfig,
    debounce_config: DebounceConfig,
    chats: Mutex<HashMap<String, mpsc::Sender<ChatJob>>>,
    handler: ChatHandler,
}

impl ChatRouter {
    pub fn new(
        config: ChatRouterConfig,
        debounce_config: DebounceConfig,
        handler: ChatHandler,
    ) -> Self {
        Self {
            config,
            debounce_config,
            chats: Mutex::new(HashMap::new()),
            handler,
        }
    }

    /// Route an inbound event to the appropriate per-chat queue.
    /// Awaits enqueue to preserve caller ordering within a single async context.
    pub async fn route(
        self: &Arc<Self>,
        account: crate::channels::model::account::ChannelAccount,
        event: InboundEvent,
    ) {
        let chat_id = extract_chat_id(&event);

        match chat_id {
            Some(id) => {
                self.route_to_chat(id, account, event).await;
            }
            // No chat_id (e.g. some PlatformEvents) → handle directly, no queuing.
            None => {
                let handler = self.handler.clone();
                let input = DebouncedInput {
                    account,
                    text: extract_text(&event),
                    primary_event: event.clone(),
                    all_events: vec![event],
                    merged_count: 1,
                };
                tokio::spawn(async move {
                    handler(input).await;
                });
            }
        }
    }

    async fn route_to_chat(
        self: &Arc<Self>,
        chat_id: String,
        account: crate::channels::model::account::ChannelAccount,
        event: InboundEvent,
    ) {
        let mut chats = self.chats.lock().await;

        // Try to send to existing queue.
        if let Some(tx) = chats.get(&chat_id) {
            if !tx.is_closed() {
                let job = ChatJob { account, event };
                match tx.send(job).await {
                    Ok(()) => return,
                    Err(send_err) => {
                        // Channel closed between is_closed and send. Recover the job.
                        chats.remove(&chat_id);
                        let recovered = send_err.0;
                        self.create_chat_queue(&mut chats, chat_id, recovered).await;
                        return;
                    }
                }
            }
            chats.remove(&chat_id);
        }

        let job = ChatJob { account, event };
        self.create_chat_queue(&mut chats, chat_id, job).await;
    }

    async fn create_chat_queue(
        self: &Arc<Self>,
        chats: &mut HashMap<String, mpsc::Sender<ChatJob>>,
        chat_id: String,
        job: ChatJob,
    ) {
        let (tx, rx) = mpsc::channel(self.config.per_chat_capacity);
        if tx.send(job).await.is_err() {
            return;
        }
        chats.insert(chat_id.clone(), tx);

        let router = self.clone();
        tokio::spawn(async move {
            router.consume_chat(chat_id, rx).await;
        });
    }

    async fn consume_chat(self: &Arc<Self>, chat_id: String, mut rx: mpsc::Receiver<ChatJob>) {
        let mut leftover: Option<ChatJob> = None;

        loop {
            let job = match leftover.take() {
                Some(j) => j,
                None => {
                    match tokio::time::timeout(self.config.idle_timeout, rx.recv()).await {
                        Ok(Some(j)) => j,
                        Ok(None) => break,
                        Err(_) => {
                            // Idle timeout. Remove from map BEFORE closing rx
                            // so new route() calls create a fresh queue instead
                            // of sending into this dying channel.
                            self.chats.lock().await.remove(&chat_id);
                            // Close the receiver. Any messages that were enqueued
                            // between the last recv timeout and the map removal
                            // are drained and re-routed below.
                            rx.close();
                            while let Some(stale_job) = rx.recv().await {
                                let input = DebouncedInput {
                                    account: stale_job.account,
                                    text: extract_text(&stale_job.event),
                                    primary_event: stale_job.event.clone(),
                                    all_events: vec![stale_job.event],
                                    merged_count: 1,
                                };
                                self.call_handler(input).await;
                            }

                            return;
                        }
                    }
                }
            };

            let result = debounce(&self.debounce_config, job, &mut rx).await;
            match result {
                DebounceResult::Ready(input) => {
                    self.call_handler(input).await;
                }
                DebounceResult::ReadyWithLeftover(input, next) => {
                    self.call_handler(*input).await;
                    leftover = Some(next);
                }
            }
        }

        // Channel closed (all senders dropped).
        self.chats.lock().await.remove(&chat_id);
    }

    /// Number of active per-chat queues.
    pub async fn active_chats(&self) -> usize {
        self.chats.lock().await.len()
    }

    async fn call_handler(&self, input: DebouncedInput) {
        let handler = self.handler.clone();
        if let Err(panic) = AssertUnwindSafe(handler(input)).catch_unwind().await {
            let msg = match panic.downcast_ref::<&str>() {
                Some(s) => s.to_string(),
                None => match panic.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "unknown panic".to_string(),
                },
            };
            crate::observability::log::slog!(error, "chat_router", "handler_panic", panic = %msg,);
        }
    }
}

fn extract_chat_id(event: &InboundEvent) -> Option<String> {
    match event {
        InboundEvent::Message(msg) if !msg.chat_id.is_empty() => Some(msg.chat_id.clone()),
        InboundEvent::PlatformEvent {
            reply_context: Some(ctx),
            ..
        } if !ctx.chat_id.is_empty() => Some(ctx.chat_id.clone()),
        InboundEvent::Callback {
            reply_context: Some(ctx),
            ..
        } if !ctx.chat_id.is_empty() => Some(ctx.chat_id.clone()),
        _ => None,
    }
}

fn extract_text(event: &InboundEvent) -> String {
    match event {
        InboundEvent::Message(msg) => msg.text.clone(),
        InboundEvent::PlatformEvent {
            event_type,
            payload,
            ..
        } => format!("[{event_type}] {payload}"),
        InboundEvent::Callback { data, .. } => data.clone(),
    }
}
