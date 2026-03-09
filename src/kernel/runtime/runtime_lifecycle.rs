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

        let handle = self.sync_handle.write().take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }

        *self.status.write() = RuntimeStatus::Stopped;
        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "runtime stopped"
        );
        Ok(())
    }
}
