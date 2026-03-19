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

        // Abort fire-and-forget background tasks immediately — no data to preserve.
        if let Some(handle) = self.sync_handle.write().take() {
            handle.abort();
        }
        if let Some(handle) = self.directive_handle.write().take() {
            handle.abort();
        }
        if let Some(handle) = self.heartbeat_handle.write().take() {
            handle.abort();
        }

        // Lease release and usage flush have side-effects — run them in parallel.
        let lease_fut = async {
            let lease = self.lease_handle.write().take();
            if let Some(handle) = lease {
                // Abort scan loops immediately — no need to wait for the
                // current scan_once to finish; release_all() below handles
                // lease cleanup independently.
                handle.abort_all();
                handle.release_all().await;
            }
        };

        let usage_fut = async {
            if let Ok(agent_ids) = self.databases.list_agent_ids().await {
                let llm = self.llm.read().clone();
                let futs: Vec<_> = agent_ids
                    .iter()
                    .filter_map(|agent_id| {
                        let pool = self.databases.agent_pool(agent_id).ok()?;
                        let store = AgentStore::new(pool, llm.clone());
                        let id = agent_id.clone();
                        Some(async move {
                            if let Err(e) = store.usage_flush().await {
                                tracing::warn!(agent_id = %id, error = %e, "failed to flush usage on shutdown");
                            }
                        })
                    })
                    .collect();
                futures::future::join_all(futs).await;
            }
        };

        // Best-effort: don't let slow DB calls delay shutdown.
        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(lease_fut, usage_fut);
        })
        .await
        .is_err()
        {
            tracing::warn!("lease release / usage flush timed out after 2s, skipping");
        }

        self.supervisor.stop_all().await;
        if let Some(ref svc) = self.cluster {
            svc.deregister().await;
        }

        self.trace_writer.shutdown().await;

        *self.status.write() = RuntimeStatus::Stopped;
        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "runtime stopped"
        );
        Ok(())
    }
}
