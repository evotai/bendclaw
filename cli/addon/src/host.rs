//! NAPI host bridge: routes engine tool delegations back to the TypeScript CLI.
//!
//! This generalizes the former bespoke `ask.rs`. Any host-owned tool (ask_user,
//! plan, or a user extension's tool) the engine invokes is forwarded to JS as a
//! `host_tool_call` event and answered via `NapiRun::respond_host_tool`.
//!
//! Multiple host tools can be in flight at once (parallel tool execution), so
//! responders are keyed by `tool_call_id` rather than a single slot.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use evot_engine::host::HostBridge;
use evot_engine::host::HostError;
use evot_engine::host::HostToolCall;
use evot_engine::host::HostToolResponse;
use evot_engine::host::HostToolSpec;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::oneshot;
use tokio::sync::Semaphore;

/// Result carried back from JS for a single host tool call.
type HostResult = std::result::Result<HostToolResponse, String>;

/// Map of in-flight tool calls awaiting a JS response, keyed by tool_call_id.
pub(crate) type HostResponders = Arc<Mutex<HashMap<String, oneshot::Sender<HostResult>>>>;

/// The engine-facing bridge. Serializes each call as a synthetic event, parks a
/// responder, and blocks until JS answers.
pub(crate) struct NapiHostBridge {
    event_tx: tokio_mpsc::Sender<String>,
    responders: HostResponders,
    capacity: Arc<Semaphore>,
}

impl NapiHostBridge {
    pub(crate) fn new(event_tx: tokio_mpsc::Sender<String>, responders: HostResponders) -> Self {
        Self {
            event_tx,
            responders,
            capacity: Arc::new(Semaphore::new(32)),
        }
    }
}

#[async_trait]
impl HostBridge for NapiHostBridge {
    async fn execute_tool(
        &self,
        call: HostToolCall,
    ) -> std::result::Result<HostToolResponse, HostError> {
        let _permit = self
            .capacity
            .acquire()
            .await
            .map_err(|_| HostError::Closed)?;
        let event = evot::contracts::HostEvent::ToolCall {
            tool_name: call.tool_name,
            tool_call_id: call.tool_call_id.clone(),
            arguments: call.arguments,
        };
        let json_str = serde_json::to_string(&event)
            .map_err(|e| HostError::Failed(format!("serialize host_tool_call: {e}")))?;

        let (resp_tx, resp_rx) = oneshot::channel::<HostResult>();
        {
            let mut guard = self.responders.lock().map_err(|_| HostError::Closed)?;
            guard.insert(call.tool_call_id.clone(), resp_tx);
        }

        let _pending = PendingResponse {
            id: call.tool_call_id,
            responders: self.responders.clone(),
        };
        if self.event_tx.send(json_str).await.is_err() {
            return Err(HostError::Closed);
        }

        match resp_rx.await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(msg)) => Err(HostError::Failed(msg)),
            Err(_) => Err(HostError::Closed),
        }
    }
}

// A cancelled host future must remove its response slot, even when JS never
// replies. Keep the lock synchronous and never hold it across an await.
struct PendingResponse {
    id: String,
    responders: HostResponders,
}
impl Drop for PendingResponse {
    fn drop(&mut self) {
        let mut guard = match self.responders.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.remove(&self.id);
    }
}

/// Parse the host tool specs the TS side registered, passed as a JSON array.
///
/// A `None` or empty payload yields no specs — the run then carries only
/// engine-owned built-in tools.
pub(crate) fn parse_host_tool_specs(json: Option<&str>) -> Result<Vec<HostToolSpec>, String> {
    match json {
        None => Ok(Vec::new()),
        Some(s) if s.trim().is_empty() => Ok(Vec::new()),
        Some(s) => serde_json::from_str::<Vec<HostToolSpec>>(s)
            .map_err(|e| format!("parse host tool specs: {e}")),
    }
}
