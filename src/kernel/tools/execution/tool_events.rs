//! Event emission for tool lifecycle.
//!
//! Only emits Event::ToolStart / Event::ToolEnd via the channel. Nothing else.

use tokio::sync::mpsc;

use super::parsed_tool_call::DispatchOutcome;
use super::parsed_tool_call::ParsedToolCall;
use super::tool_result::ToolCallResult;
use crate::kernel::run::event::Event;

pub struct EventEmitter {
    tx: mpsc::Sender<Event>,
}

impl EventEmitter {
    pub fn new(tx: mpsc::Sender<Event>) -> Self {
        Self { tx }
    }

    pub async fn tool_start(&self, parsed: &ParsedToolCall) {
        let _ = self
            .tx
            .send(Event::ToolStart {
                tool_call_id: parsed.call.id.clone(),
                name: parsed.call.name.clone(),
                arguments: parsed.arguments.clone(),
            })
            .await;
    }

    pub async fn tool_end(&self, outcome: &DispatchOutcome) {
        let p = &outcome.parsed;
        let meta = outcome.result.operation().clone();
        let (success, output) = match &outcome.result {
            ToolCallResult::Success(out, _) => (true, out.clone()),
            ToolCallResult::ToolError(msg, _) | ToolCallResult::InfraError(msg, _) => {
                (false, format!("Error: {msg}"))
            }
        };
        let _ = self
            .tx
            .send(Event::ToolEnd {
                tool_call_id: p.call.id.clone(),
                name: p.call.name.clone(),
                success,
                output,
                operation: meta,
            })
            .await;
    }
}
