use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::kernel::channels::egress::backpressure::BackpressureConfig;
use crate::kernel::channels::egress::backpressure::BackpressureSender;
use crate::kernel::channels::model::account::ChannelAccount;
use crate::kernel::channels::model::message::InboundEvent;
use crate::kernel::channels::model::status::ChannelStatus;
use crate::kernel::channels::routing::chat_router::ChatRouter;
use crate::kernel::channels::runtime::channel_registry::ChannelRegistry;
use crate::kernel::channels::runtime::channel_trait::InboundKind;

struct ReceiverSlot {
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

pub struct ChannelSupervisor {
    registry: Arc<ChannelRegistry>,
    receivers: Mutex<HashMap<String, ReceiverSlot>>,
    router: Arc<ChatRouter>,
    status: Arc<ChannelStatus>,
}

impl ChannelSupervisor {
    pub fn new(
        registry: Arc<ChannelRegistry>,
        router: Arc<ChatRouter>,
        status: Arc<ChannelStatus>,
    ) -> Self {
        Self {
            registry,
            receivers: Mutex::new(HashMap::new()),
            router,
            status,
        }
    }

    pub fn status(&self) -> &Arc<ChannelStatus> {
        &self.status
    }

    pub async fn start(&self, account: &ChannelAccount) -> Result<()> {
        let entry = match self.registry.get(&account.channel_type) {
            Some(e) => e,
            None => return Ok(()),
        };

        let factory = match &entry.inbound {
            InboundKind::Receiver(f) => f.clone(),
            _ => return Ok(()),
        };

        self.stop(&account.channel_account_id).await;

        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<InboundEvent>(1024);
        let bp_sender = BackpressureSender::new(
            event_tx,
            BackpressureConfig::default(),
            self.status.clone(),
            account.channel_account_id.clone(),
        );

        let stale_threshold = entry
            .plugin
            .capabilities()
            .stale_event_threshold
            .unwrap_or_else(ChannelStatus::default_stale_threshold);
        self.status.reset(
            &account.channel_account_id,
            account.config.clone(),
            stale_threshold,
        );

        let handle = factory.spawn(account, bp_sender, cancel.clone()).await?;

        let router = self.router.clone();
        let account_clone = account.clone();
        crate::base::spawn_fire_and_forget("channel_event_consumer", async move {
            while let Some(event) = event_rx.recv().await {
                router.route(account_clone.clone(), event).await;
            }
        });

        self.receivers
            .lock()
            .await
            .insert(account.channel_account_id.clone(), ReceiverSlot {
                cancel,
                handle,
            });

        Ok(())
    }

    pub async fn stop(&self, channel_account_id: &str) {
        if let Some(slot) = self.receivers.lock().await.remove(channel_account_id) {
            slot.cancel.cancel();
            slot.handle.abort();
        }
        self.status.clear(channel_account_id);
    }

    pub async fn stop_all(&self) {
        let mut map = self.receivers.lock().await;
        for (id, slot) in map.drain() {
            slot.cancel.cancel();
            slot.handle.abort();
            self.status.clear(&id);
        }
    }

    pub async fn is_running(&self, channel_account_id: &str) -> bool {
        self.receivers.lock().await.contains_key(channel_account_id)
    }

    pub async fn is_alive(&self, channel_account_id: &str) -> bool {
        match self.receivers.lock().await.get(channel_account_id) {
            Some(slot) => !slot.handle.is_finished(),
            None => false,
        }
    }

    pub async fn tracked_account_ids(&self) -> Vec<String> {
        self.receivers.lock().await.keys().cloned().collect()
    }
}
