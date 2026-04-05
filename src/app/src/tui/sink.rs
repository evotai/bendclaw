use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::Result;
use crate::request::EventSink;
use crate::storage::model::RunEvent;
use crate::tui::state::TuiEvent;

pub struct TuiSink {
    tx: UnboundedSender<TuiEvent>,
}

impl TuiSink {
    pub fn new(tx: UnboundedSender<TuiEvent>) -> Arc<Self> {
        Arc::new(Self { tx })
    }
}

#[async_trait]
impl EventSink for TuiSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        let _ = self.tx.send(TuiEvent::RunEvent(event));
        Ok(())
    }
}
