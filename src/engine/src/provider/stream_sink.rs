//! Provider delivery port. Built-in streaming decoders await capacity instead
//! of running ahead of the runner. Legacy providers retain their public API.
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::traits::StreamEvent;

#[derive(Clone)]
pub enum StreamSink {
    Legacy(mpsc::UnboundedSender<StreamEvent>),
    Bounded {
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    },
}
impl StreamSink {
    pub async fn send(&self, event: StreamEvent) -> Result<(), ()> {
        match self {
            Self::Legacy(tx) => tx.send(event).map_err(|_| ()),
            Self::Bounded { tx, cancel } => tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(()),
                result = tx.send(event) => {
                    if result.is_err() { cancel.cancel(); }
                    result.map_err(|_| ())
                }
            },
        }
    }
}
