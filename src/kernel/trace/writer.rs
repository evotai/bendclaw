//! Background trace writer — async queue for fire-and-forget DB writes.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;

const CHANNEL_CAPACITY: usize = 1024;
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH_SIZE: usize = 20;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(200);

pub enum TraceOp {
    InsertTrace {
        repo: Arc<TraceRepo>,
        record: TraceRecord,
    },
    UpdateTraceCompleted {
        repo: Arc<TraceRepo>,
        trace_id: String,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_cost: f64,
    },
    UpdateTraceFailed {
        repo: Arc<TraceRepo>,
        trace_id: String,
        duration_ms: u64,
    },
    AppendSpan {
        repo: Arc<SpanRepo>,
        record: SpanRecord,
    },
    Shutdown,
}

struct TraceWriterInner {
    tx: mpsc::Sender<TraceOp>,
    handle: Mutex<Option<JoinHandle<()>>>,
    shutting_down: AtomicBool,
}

#[derive(Clone)]
pub struct TraceWriter {
    inner: Arc<TraceWriterInner>,
}

impl TraceWriter {
    /// Create a new writer and spawn the background drain task.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let handle = tokio::spawn(drain_loop(rx));
        Self {
            inner: Arc::new(TraceWriterInner {
                tx,
                handle: Mutex::new(Some(handle)),
                shutting_down: AtomicBool::new(false),
            }),
        }
    }

    /// Create a no-op writer for tests without a Tokio runtime.
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self {
            inner: Arc::new(TraceWriterInner {
                tx,
                handle: Mutex::new(None),
                shutting_down: AtomicBool::new(true),
            }),
        }
    }

    /// Send an operation to the background queue. Never blocks; drops on full.
    pub fn send(&self, op: TraceOp) {
        if self.inner.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        if self.inner.tx.try_send(op).is_err() {
            tracing::warn!("trace writer queue full, dropping op");
        }
    }

    /// Fast shutdown: stop accepting new ops and drop any pending queued writes.
    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::Relaxed);
        tracing::info!("trace writer shutting down");

        let Some(mut handle) = self.inner.handle.lock().take() else {
            return;
        };

        let _ = self.inner.tx.try_send(TraceOp::Shutdown);
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut handle)
            .await
            .is_err()
        {
            tracing::warn!(
                timeout_ms = SHUTDOWN_TIMEOUT.as_millis() as u64,
                "trace writer shutdown timed out, aborting"
            );
            handle.abort();
            let _ = handle.await;
        }
    }
}

struct SpanBatch {
    repo: Arc<SpanRepo>,
    records: Vec<SpanRecord>,
}

async fn drain_loop(mut rx: mpsc::Receiver<TraceOp>) {
    let mut span_batches: Vec<SpanBatch> = Vec::new();
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            op = rx.recv() => {
                match op {
                    Some(TraceOp::Shutdown) => {
                        let dropped = drop_pending_ops(&mut rx, &mut span_batches);
                        tracing::info!(dropped, "trace writer stopped");
                        return;
                    }
                    Some(op) => {
                        process_op(op, &mut span_batches).await;
                        let total: usize = span_batches.iter().map(|b| b.records.len()).sum();
                        if total >= MAX_BATCH_SIZE {
                            flush_spans(&mut span_batches).await;
                        }
                    }
                    None => {
                        flush_spans(&mut span_batches).await;
                        tracing::info!("trace writer stopped");
                        return;
                    }
                }
            }
            _ = interval.tick() => {
                flush_spans(&mut span_batches).await;
            }
        }
    }
}

fn drop_pending_ops(rx: &mut mpsc::Receiver<TraceOp>, batches: &mut Vec<SpanBatch>) -> usize {
    let mut dropped: usize = batches.iter().map(|batch| batch.records.len()).sum();
    batches.clear();

    while let Ok(op) = rx.try_recv() {
        dropped += match op {
            TraceOp::AppendSpan { .. }
            | TraceOp::InsertTrace { .. }
            | TraceOp::UpdateTraceCompleted { .. }
            | TraceOp::UpdateTraceFailed { .. } => 1,
            TraceOp::Shutdown => 0,
        };
    }

    dropped
}

async fn process_op(op: TraceOp, span_batches: &mut Vec<SpanBatch>) {
    match op {
        TraceOp::InsertTrace { repo, record } => {
            if let Err(e) = repo.insert(&record).await {
                tracing::warn!(error = %e, "trace writer: failed to insert trace");
            }
        }
        TraceOp::UpdateTraceCompleted {
            repo,
            trace_id,
            duration_ms,
            input_tokens,
            output_tokens,
            total_cost,
        } => {
            if let Err(e) = repo
                .update_completed(
                    &trace_id,
                    duration_ms,
                    input_tokens,
                    output_tokens,
                    total_cost,
                )
                .await
            {
                tracing::warn!(error = %e, "trace writer: failed to complete trace");
            }
        }
        TraceOp::UpdateTraceFailed {
            repo,
            trace_id,
            duration_ms,
        } => {
            if let Err(e) = repo.update_failed(&trace_id, duration_ms).await {
                tracing::warn!(error = %e, "trace writer: failed to fail trace");
            }
        }
        TraceOp::AppendSpan { repo, record } => {
            if let Some(batch) = span_batches
                .iter_mut()
                .find(|b| Arc::ptr_eq(&b.repo, &repo))
            {
                batch.records.push(record);
            } else {
                span_batches.push(SpanBatch {
                    repo,
                    records: vec![record],
                });
            }
        }
        TraceOp::Shutdown => {}
    }
}

async fn flush_spans(batches: &mut Vec<SpanBatch>) {
    for batch in batches.drain(..) {
        for record in &batch.records {
            if let Err(e) = batch.repo.append(record).await {
                tracing::warn!(error = %e, "trace writer: failed to append span");
            }
        }
    }
}
