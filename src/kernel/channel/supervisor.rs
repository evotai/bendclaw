use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::plugin::InboundKind;
use crate::kernel::channel::registry::ChannelRegistry;

struct ReceiverSlot {
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

pub struct ChannelSupervisor {
    registry: Arc<ChannelRegistry>,
    receivers: Mutex<HashMap<String, ReceiverSlot>>,
    event_handler: Arc<dyn Fn(ChannelAccount, InboundEvent) + Send + Sync>,
}

impl ChannelSupervisor {
    pub fn new(
        registry: Arc<ChannelRegistry>,
        event_handler: Arc<dyn Fn(ChannelAccount, InboundEvent) + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            receivers: Mutex::new(HashMap::new()),
            event_handler,
        }
    }

    /// Idempotent: stops any existing receiver for this account, then starts a new one.
    /// No-op for non-Receiver inbound kinds.
    pub async fn start(&self, account: &ChannelAccount) -> Result<()> {
        let entry = match self.registry.get(&account.channel_type) {
            Some(e) => e,
            None => return Ok(()),
        };

        let factory = match &entry.inbound {
            InboundKind::Receiver(f) => f.clone(),
            _ => return Ok(()),
        };

        // Stop any existing slot first.
        self.stop(&account.channel_account_id).await;

        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<InboundEvent>();

        let handle = factory
            .spawn(account, event_tx, cancel.clone())
            .await?;

        // Spawn consumer that dispatches events to the handler.
        let handler = self.event_handler.clone();
        let account_clone = account.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let h = handler.clone();
                let a = account_clone.clone();
                tokio::spawn(async move { h(a, event) });
            }
        });

        self.receivers.lock().await.insert(
            account.channel_account_id.clone(),
            ReceiverSlot { cancel, handle },
        );

        Ok(())
    }

    pub async fn stop(&self, channel_account_id: &str) {
        if let Some(slot) = self
            .receivers
            .lock()
            .await
            .remove(channel_account_id)
        {
            slot.cancel.cancel();
            slot.handle.abort();
        }
    }

    pub async fn stop_all(&self) {
        let mut map = self.receivers.lock().await;
        for (_, slot) in map.drain() {
            slot.cancel.cancel();
            slot.handle.abort();
        }
    }

    pub async fn is_running(&self, channel_account_id: &str) -> bool {
        self.receivers
            .lock()
            .await
            .contains_key(channel_account_id)
    }
}
