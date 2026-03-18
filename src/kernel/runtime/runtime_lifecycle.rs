use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
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
            if tokio::time::timeout(shutdown_timeout, handle)
                .await
                .is_err()
            {
                tracing::warn!("sync task did not finish within timeout");
            }
        }

        let lease = self.lease_handle.write().take();
        if let Some(handle) = lease {
            // Wait for scan loops to exit. The cancel check before the claim
            // branch in scan_once minimizes (but doesn't eliminate) the window
            // for new claims — cooperative cancellation can't be fully atomic.
            if tokio::time::timeout(shutdown_timeout, handle.join())
                .await
                .is_err()
            {
                tracing::warn!("lease scan loops did not finish within timeout, aborting");
                handle.abort_all();
            }
            // Each resource type decides via safe_to_release() whether its
            // leases can be released now. Channels always release immediately;
            // tasks only release if all workers have drained (checked via
            // activity_tracker). No global drain wait — avoids delaying
            // channel failover when task workers are slow.
            handle.release_all().await;
        }
        self.supervisor.stop_all().await;

        // Cluster cleanup: cancel heartbeat and deregister
        let hb_handle = self.heartbeat_handle.write().take();
        if let Some(handle) = hb_handle {
            if tokio::time::timeout(shutdown_timeout, handle)
                .await
                .is_err()
            {
                tracing::warn!("heartbeat task did not finish within timeout");
            }
        }
        if let Some(ref svc) = self.cluster {
            svc.deregister().await;
        }

        let directive_handle = self.directive_handle.write().take();
        if let Some(handle) = directive_handle {
            if tokio::time::timeout(shutdown_timeout, handle)
                .await
                .is_err()
            {
                tracing::warn!("directive task did not finish within timeout");
            }
        }

        // Flush buffered usage records for all agents before stopping.
        if let Ok(agent_ids) = self.databases.list_agent_ids().await {
            let llm = self.llm.read().clone();
            for agent_id in &agent_ids {
                if let Ok(pool) = self.databases.agent_pool(agent_id) {
                    let store = AgentStore::new(pool, llm.clone());
                    if let Err(e) = store.usage_flush().await {
                        tracing::warn!(agent_id, error = %e, "failed to flush usage on shutdown");
                    }
                }
            }
        }

        // Stop trace writer quickly; pending trace ops may be dropped on shutdown.
        self.trace_writer.shutdown().await;

        *self.status.write() = RuntimeStatus::Stopped;
        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "runtime stopped"
        );
        Ok(())
    }
}
