use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::runtime::Runtime;
use crate::kernel::runtime::RuntimeStatus;

impl Runtime {
    pub fn status(&self) -> RuntimeStatus {
        *self.status.read()
    }

    pub(crate) fn require_ready(&self) -> Result<()> {
        let s = self.status();
        if s != RuntimeStatus::Ready {
            return Err(ErrorCode::internal(format!(
                "runtime is not ready (status: {s:?})"
            )));
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!(status = ?self.status(), "runtime shutting down");
        let t0 = std::time::Instant::now();
        *self.status.write() = RuntimeStatus::ShuttingDown;

        self.sessions.close_all().await;
        self.sync_cancel.cancel();

        let shutdown_timeout = std::time::Duration::from_secs(10);

        let handle = self.sync_handle.write().take();
        if let Some(handle) = handle {
            if tokio::time::timeout(shutdown_timeout, handle).await.is_err() {
                tracing::warn!("sync task did not finish within timeout");
            }
        }

        let sched_handle = self.scheduler_handle.write().take();
        if let Some(handle) = sched_handle {
            if tokio::time::timeout(shutdown_timeout, handle).await.is_err() {
                tracing::warn!("scheduler task did not finish within timeout");
            }
        }

        // Cluster cleanup: cancel heartbeat and deregister
        let hb_handle = self.heartbeat_handle.write().take();
        if let Some(handle) = hb_handle {
            if tokio::time::timeout(shutdown_timeout, handle).await.is_err() {
                tracing::warn!("heartbeat task did not finish within timeout");
            }
        }
        if let Some(ref svc) = self.cluster {
            svc.deregister().await;
        }

        let directive_handle = self.directive_handle.write().take();
        if let Some(handle) = directive_handle {
            if tokio::time::timeout(shutdown_timeout, handle).await.is_err() {
                tracing::warn!("directive task did not finish within timeout");
            }
        }

        *self.status.write() = RuntimeStatus::Stopped;
        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "runtime stopped"
        );
        Ok(())
    }
}
