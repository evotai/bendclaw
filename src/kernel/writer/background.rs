//! Generic background writer — async queue for fire-and-forget writes.
//!
//! Shared infrastructure for `TraceWriter`, `PersistWriter`, and any future
//! background write needs. Each consumer defines its own `Op` enum and
//! handler function.

use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::observability::log::slog;

const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);

struct Inner<Op> {
    tx: mpsc::Sender<Op>,
    handle: Mutex<Option<JoinHandle<()>>>,
    shutting_down: AtomicBool,
    name: &'static str,
}

pub struct BackgroundWriter<Op> {
    inner: Arc<Inner<Op>>,
}

impl<Op> Clone for BackgroundWriter<Op> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<Op: Send + 'static> BackgroundWriter<Op> {
    /// Spawn a background drain loop.
    ///
    /// `handler` is called for each op. Return `true` to continue, `false` to stop.
    pub fn spawn<H, Fut>(name: &'static str, capacity: usize, handler: H) -> Self
    where
        H: FnMut(Op) -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(capacity);
        let handle =
            crate::types::spawn_named("background_drain_loop", drain_loop(name, rx, handler));
        Self {
            inner: Arc::new(Inner {
                tx,
                handle: Mutex::new(Some(handle)),
                shutting_down: AtomicBool::new(false),
                name,
            }),
        }
    }

    /// Build from pre-existing channel + handle.
    /// For consumers with custom drain loops (e.g. TraceWriter with batching).
    pub fn from_parts(name: &'static str, tx: mpsc::Sender<Op>, handle: JoinHandle<()>) -> Self {
        Self {
            inner: Arc::new(Inner {
                tx,
                handle: Mutex::new(Some(handle)),
                shutting_down: AtomicBool::new(false),
                name,
            }),
        }
    }

    /// Create a no-op writer that silently drops all ops. For tests.
    pub fn noop(name: &'static str) -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self {
            inner: Arc::new(Inner {
                tx,
                handle: Mutex::new(None),
                shutting_down: AtomicBool::new(true),
                name,
            }),
        }
    }

    /// Send an op to the background queue. Never blocks; drops on full.
    pub fn send(&self, op: Op) {
        if self.inner.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        if self.inner.tx.try_send(op).is_err() {
            slog!(warn, "writer", "queue_full", writer = self.inner.name,);
        }
    }

    /// Graceful shutdown: signal the drain loop and wait (with timeout).
    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::Relaxed);

        let Some(mut handle) = self.inner.handle.lock().take() else {
            return;
        };

        // Close sender side so drain_loop sees None from recv()
        // (we can't send a sentinel without knowing Op's shape)
        drop(self.inner.tx.clone()); // drop our clone; other clones may still exist

        if tokio::time::timeout(DEFAULT_SHUTDOWN_TIMEOUT, &mut handle)
            .await
            .is_err()
        {
            slog!(
                warn,
                "writer",
                "shutdown_timeout",
                writer = self.inner.name,
                timeout_ms = DEFAULT_SHUTDOWN_TIMEOUT.as_millis() as u64,
            );
            handle.abort();
            let _ = handle.await;
        }
    }

    pub fn is_shutting_down(&self) -> bool {
        self.inner.shutting_down.load(Ordering::Relaxed)
    }
}

async fn drain_loop<Op, H, Fut>(_name: &'static str, mut rx: mpsc::Receiver<Op>, mut handler: H)
where
    H: FnMut(Op) -> Fut,
    Fut: Future<Output = bool>,
{
    loop {
        match rx.recv().await {
            Some(op) => {
                if !handler(op).await {
                    return;
                }
            }
            None => {
                return;
            }
        }
    }
}
