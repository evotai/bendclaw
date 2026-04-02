//! Background trace writer — delegates to `BackgroundWriter<TraceOp>`.

use std::sync::Arc;
use std::time::Duration;

use crate::kernel::trace::diagnostics;
use crate::kernel::writer::BackgroundWriter;
use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;

const CHANNEL_CAPACITY: usize = 1024;
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_BATCH_SIZE: usize = 20;

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

#[derive(Clone)]
pub struct TraceWriter {
    inner: BackgroundWriter<TraceOp>,
}

impl TraceWriter {
    pub fn spawn() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);
        let handle = crate::types::spawn_named("trace_drain_loop", trace_drain_loop(rx));

        // Build a BackgroundWriter manually using the internal spawn with
        // a simple forwarding handler, OR we can use the raw tx/rx approach.
        // Since TraceWriter has custom batching logic (span batches + flush
        // interval), we keep its own drain loop and wrap with a thin struct.
        Self {
            inner: BackgroundWriter::from_parts("trace", tx, handle),
        }
    }

    pub fn noop() -> Self {
        Self {
            inner: BackgroundWriter::noop("trace"),
        }
    }

    pub fn send(&self, op: TraceOp) {
        self.inner.send(op);
    }

    pub async fn shutdown(&self) {
        self.inner.shutdown().await;
    }
}

// ── Custom drain loop for trace batching ─────────────────────────────────

struct SpanBatch {
    repo: Arc<SpanRepo>,
    records: Vec<SpanRecord>,
}

async fn trace_drain_loop(mut rx: tokio::sync::mpsc::Receiver<TraceOp>) {
    let mut span_batches: Vec<SpanBatch> = Vec::new();
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            op = rx.recv() => {
                match op {
                    Some(TraceOp::Shutdown) => {
                        let _ = drop_pending(&mut rx, &mut span_batches);
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

fn drop_pending(
    rx: &mut tokio::sync::mpsc::Receiver<TraceOp>,
    batches: &mut Vec<SpanBatch>,
) -> usize {
    let mut dropped: usize = batches.iter().map(|b| b.records.len()).sum();
    batches.clear();
    while let Ok(op) = rx.try_recv() {
        dropped += match op {
            TraceOp::Shutdown => 0,
            _ => 1,
        };
    }
    dropped
}

async fn process_op(op: TraceOp, span_batches: &mut Vec<SpanBatch>) {
    match op {
        TraceOp::InsertTrace { repo, record } => {
            if let Err(e) = repo.insert(&record).await {
                diagnostics::log_trace_insert_failed(&e);
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
                diagnostics::log_trace_update_failed(&e);
            }
        }
        TraceOp::UpdateTraceFailed {
            repo,
            trace_id,
            duration_ms,
        } => {
            if let Err(e) = repo.update_failed(&trace_id, duration_ms).await {
                diagnostics::log_trace_update_failed(&e);
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
                diagnostics::log_trace_append_failed(&e);
            }
        }
    }
}
