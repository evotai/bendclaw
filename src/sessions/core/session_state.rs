use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::sessions::Message;

pub enum SessionState {
    Idle,
    Running {
        run_id: String,
        cancel: CancellationToken,
        started_at: Instant,
        iteration: Arc<AtomicU32>,
        inbox_tx: mpsc::Sender<Message>,
    },
}

pub type SharedSessionState = Arc<Mutex<SessionState>>;
