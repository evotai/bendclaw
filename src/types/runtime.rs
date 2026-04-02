use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use tokio::task::JoinHandle;

use crate::observability::log::slog;

/// DB discover / migration queries — moderate limit.
pub const CONCURRENCY_DB: usize = 8;

/// Tool call dispatch — higher limit, these are mostly HTTP I/O.
pub const CONCURRENCY_TOOLS: usize = 16;

/// Shutdown / release paths — generous bound to drain quickly.
pub const CONCURRENCY_SHUTDOWN: usize = 32;

/// Execute futures with bounded concurrency using a semaphore.
///
/// At most `max_concurrent` futures run simultaneously.
/// All futures are polled on the current task (no spawning), so borrowed data is fine.
pub async fn join_bounded<I, F, T>(futures: I, max_concurrent: usize) -> Vec<T>
where
    I: IntoIterator<Item = F>,
    F: Future<Output = T>,
{
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let wrapped: Vec<_> = futures
        .into_iter()
        .map(|fut| {
            let permit = sem.clone();
            async move {
                // unwrap is safe: we never close the semaphore
                let _permit = permit.acquire().await.unwrap_or_else(|_| unreachable!());
                fut.await
            }
        })
        .collect();
    futures::future::join_all(wrapped).await
}

/// Spawn a named tokio task that catches panics and logs them instead of
/// propagating. Inspired by databend's `spawn_named` + `catch_unwind`.
pub fn spawn_named<F>(name: &'static str, fut: F) -> JoinHandle<()>
where F: Future<Output = ()> + Send + 'static {
    tokio::spawn(async move {
        if let Err(panic) = AssertUnwindSafe(fut).catch_unwind().await {
            let msg = match panic.downcast_ref::<&str>() {
                Some(s) => s.to_string(),
                None => match panic.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "unknown panic payload".to_string(),
                },
            };
            slog!(error, "runtime", "panicked", task = name, panic = %msg,);
        }
    })
}

/// Spawn a fire-and-forget task. The JoinHandle is intentionally dropped.
pub fn spawn_fire_and_forget<F>(name: &'static str, fut: F)
where F: Future<Output = ()> + Send + 'static {
    drop(spawn_named(name, fut));
}
