use crate::agent_store::AgentStore;
use crate::runtime::diagnostics;
use crate::runtime::Runtime;
use crate::runtime::RuntimeStatus;
use crate::types::ErrorCode;
use crate::types::Result;

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
                                diagnostics::log_runtime_flush_failed(&id, &e);
                            }
                        })
                    })
                    .collect();
                crate::types::runtime::join_bounded(
                    futs,
                    crate::types::runtime::CONCURRENCY_SHUTDOWN,
                )
                .await;
            }
        };

        // Best-effort: don't let slow DB calls delay shutdown.
        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(lease_fut, usage_fut);
        })
        .await
        .is_err()
        {
            diagnostics::log_runtime_shutdown_timeout();
        }

        self.supervisor.stop_all().await;
        let cluster_svc = self.cluster.read().clone();
        if let Some(ref svc) = cluster_svc {
            svc.deregister().await;
        }

        // Drain background writers in parallel.
        tokio::join!(
            self.persist_writer.shutdown(),
            self.trace_writer.shutdown(),
            self.channel_message_writer.shutdown(),
            self.tool_writer.shutdown(),
        );

        *self.status.write() = RuntimeStatus::Stopped;
        diagnostics::log_runtime_stopped(t0.elapsed().as_millis() as u64);
        Ok(())
    }
}
