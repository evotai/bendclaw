use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;

use crate::client::BendclawClient;
use crate::types::ErrorCode;
use crate::types::Result;

/// Whether a run status string represents a terminal state.
fn is_terminal(status: &str) -> bool {
    matches!(status, "COMPLETED" | "ERROR" | "CANCELLED")
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchEntry {
    pub dispatch_id: String,
    pub node_id: String,
    pub endpoint: String,
    pub agent_id: String,
    pub run_id: String,
    #[serde(skip)]
    pub user_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Trace ID of the dispatching (parent) run, for distributed trace correlation.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parent_trace_id: String,
}

/// Per-session in-memory state tracking dispatched subtasks to peer nodes.
pub struct DispatchTable {
    entries: Mutex<HashMap<String, DispatchEntry>>,
    client: Arc<BendclawClient>,
    poll_interval: Duration,
}

impl DispatchTable {
    pub fn new(client: Arc<BendclawClient>) -> Self {
        Self::with_poll_interval(client, Duration::from_secs(2))
    }

    pub fn with_poll_interval(client: Arc<BendclawClient>, poll_interval: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            client,
            poll_interval,
        }
    }

    /// Dispatch a subtask to a remote node. Returns a dispatch_id (ULID).
    #[allow(clippy::too_many_arguments)]
    pub async fn dispatch(
        &self,
        node_id: &str,
        endpoint: &str,
        agent_id: &str,
        input: &str,
        user_id: &str,
        parent_run_id: Option<&str>,
        trace_id: Option<&str>,
        origin_node_id: Option<&str>,
    ) -> Result<String> {
        let dispatch_id = ulid::Ulid::new().to_string();
        let resp = self
            .client
            .create_run(
                endpoint,
                agent_id,
                input,
                user_id,
                parent_run_id,
                trace_id,
                origin_node_id,
            )
            .await?;

        let remote_run_id = resp.id;
        let entry = DispatchEntry {
            dispatch_id: dispatch_id.clone(),
            node_id: node_id.to_string(),
            endpoint: endpoint.to_string(),
            agent_id: agent_id.to_string(),
            run_id: remote_run_id.clone(),
            user_id: user_id.to_string(),
            status: "RUNNING".to_string(),
            output: None,
            error: None,
            parent_trace_id: trace_id.unwrap_or_default().to_string(),
        };
        self.entries.lock().insert(dispatch_id.clone(), entry);
        Ok(dispatch_id)
    }

    /// Poll remote nodes until all dispatches are terminal or timeout is reached.
    pub async fn collect(
        &self,
        dispatch_ids: &[String],
        timeout: Duration,
    ) -> Result<Vec<DispatchEntry>> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let mut all_done = true;

            for id in dispatch_ids {
                let entry = {
                    let entries = self.entries.lock();
                    entries.get(id).cloned()
                };
                let Some(entry) = entry else {
                    return Err(ErrorCode::cluster_collect(format!(
                        "unknown dispatch_id: {id}"
                    )));
                };

                if !is_terminal(&entry.status) {
                    all_done = false;
                    match self
                        .client
                        .get_run(
                            &entry.endpoint,
                            &entry.agent_id,
                            &entry.run_id,
                            &entry.user_id,
                        )
                        .await
                    {
                        Ok(resp) => {
                            let mut entries = self.entries.lock();
                            if let Some(e) = entries.get_mut(id) {
                                e.status = resp.status;
                                if !resp.output.is_empty() {
                                    e.output = Some(resp.output);
                                }
                                if !resp.error.is_empty() {
                                    e.error = Some(resp.error);
                                }
                            }
                        }
                        Err(e) => {
                            let mut entries = self.entries.lock();
                            if let Some(entry) = entries.get_mut(id) {
                                entry.status = "ERROR".to_string();
                                entry.error = Some(e.message.clone());
                            }
                        }
                    }
                }
            }

            if all_done {
                break;
            }

            if tokio::time::Instant::now() >= deadline {
                break;
            }

            tokio::time::sleep(self.poll_interval).await;
        }

        let entries = self.entries.lock();
        Ok(dispatch_ids
            .iter()
            .filter_map(|id| entries.get(id).cloned())
            .collect())
    }

    /// Get a single dispatch entry by ID.
    pub fn get(&self, dispatch_id: &str) -> Option<DispatchEntry> {
        self.entries.lock().get(dispatch_id).cloned()
    }

    /// List all dispatch entries.
    pub fn list(&self) -> Vec<DispatchEntry> {
        self.entries.lock().values().cloned().collect()
    }
}
