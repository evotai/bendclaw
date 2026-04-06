use serde_json::json;
use serde_json::Value;

use super::payload::extract_content_text;
use super::payload::AssistantPayload;
use super::payload::RequestFinishedPayload;
use super::payload::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

#[derive(Clone, Copy)]
pub struct RunEventContext<'a> {
    run_id: &'a str,
    session_id: &'a str,
    turn: u32,
}

impl<'a> RunEventContext<'a> {
    pub fn new(run_id: &'a str, session_id: &'a str, turn: u32) -> Self {
        Self {
            run_id,
            session_id,
            turn,
        }
    }

    pub fn started(&self) -> RunEvent {
        self.with_turn(0).event(RunEventKind::RunStarted, json!({}))
    }

    pub fn finished(&self, messages: &[bend_engine::AgentMessage], duration_ms: u64) -> RunEvent {
        self.event(
            RunEventKind::RunFinished,
            serde_json::to_value(RequestFinishedPayload::from_messages(
                messages,
                self.turn,
                duration_ms,
            ))
            .unwrap_or(json!({})),
        )
    }

    pub fn map(&self, event: &bend_engine::AgentEvent) -> Option<RunEvent> {
        let (kind, payload) = match event {
            bend_engine::AgentEvent::AgentStart => return None,
            bend_engine::AgentEvent::TurnStart => (RunEventKind::TurnStarted, json!({})),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Text { delta },
                ..
            } => (RunEventKind::AssistantDelta, json!({ "delta": delta })),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Thinking { delta },
                ..
            } => (
                RunEventKind::AssistantDelta,
                json!({ "thinking_delta": delta }),
            ),
            bend_engine::AgentEvent::MessageEnd { message } => (
                RunEventKind::AssistantCompleted,
                serde_json::to_value(AssistantPayload::from(message)).unwrap_or(json!({})),
            ),
            bend_engine::AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => (
                RunEventKind::ToolStarted,
                json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "args": args }),
            ),
            bend_engine::AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                partial_result,
            } => {
                let text = extract_content_text(&partial_result.content);
                (
                    RunEventKind::ToolProgress,
                    json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "text": text }),
                )
            }
            bend_engine::AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
            } => (
                RunEventKind::ToolFinished,
                serde_json::to_value(ToolResultPayload::from_result(
                    tool_call_id,
                    tool_name,
                    result,
                    *is_error,
                ))
                .unwrap_or(json!({})),
            ),
            bend_engine::AgentEvent::ProgressMessage {
                tool_call_id,
                tool_name,
                text,
            } => (
                RunEventKind::ToolProgress,
                json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "text": text }),
            ),
            bend_engine::AgentEvent::InputRejected { reason } => {
                (RunEventKind::Error, json!({ "message": reason }))
            }
            bend_engine::AgentEvent::AgentEnd { .. } => return None,
            bend_engine::AgentEvent::MessageStart { .. }
            | bend_engine::AgentEvent::TurnEnd { .. }
            | bend_engine::AgentEvent::MessageUpdate { .. } => return None,
        };

        Some(self.event(kind, payload))
    }

    fn with_turn(self, turn: u32) -> Self {
        Self { turn, ..self }
    }

    fn event(self, kind: RunEventKind, payload: Value) -> RunEvent {
        RunEvent::new(
            self.run_id.to_string(),
            self.session_id.to_string(),
            self.turn,
            kind,
            payload,
        )
    }
}
