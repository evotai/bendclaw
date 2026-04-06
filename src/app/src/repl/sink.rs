use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use super::render::build_run_summary;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_assistant_delta;
use super::render::terminal_prefixed_writeln;
use super::render::terminal_writeln;
use super::render::ToolCallSummary;
use super::render::DIM;
use super::render::RED;
use super::render::RESET;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SinkState {
    pub assistant_open: bool,
    pub assistant_prefixed: bool,
    pub streamed_assistant: bool,
    pub pending_tools: HashMap<String, ToolCallDisplay>,
}

pub struct ToolCallDisplay {
    pub name: String,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn finish_assistant_line(state: &mut SinkState) {
    if state.assistant_open {
        terminal_writeln("");
    }
    state.assistant_open = false;
    state.assistant_prefixed = false;
    state.streamed_assistant = false;
}

// ---------------------------------------------------------------------------
// ReplSink
// ---------------------------------------------------------------------------

pub struct ReplSink {
    state: Mutex<SinkState>,
}

impl ReplSink {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SinkState::default()),
        }
    }
}

impl Default for ReplSink {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventSink for ReplSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| crate::error::BendclawError::Cli("sink state lock poisoned".into()))?;

        match &event.kind {
            RunEventKind::RunStarted => {
                state.assistant_open = false;
                state.assistant_prefixed = false;
                state.streamed_assistant = false;
                state.pending_tools.clear();
            }
            RunEventKind::TurnStarted => {}
            RunEventKind::AssistantCompleted => {
                if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                    for block in payload.content {
                        match block {
                            AssistantBlock::Text { text } => {
                                if state.streamed_assistant {
                                    terminal_writeln("");
                                } else if !text.trim().is_empty() {
                                    terminal_prefixed_writeln(&text);
                                }
                                state.assistant_open = false;
                                state.assistant_prefixed = false;
                                state.streamed_assistant = false;
                            }
                            AssistantBlock::ToolCall { id, name, input } => {
                                finish_assistant_line(&mut state);
                                state.pending_tools.insert(id, ToolCallDisplay {
                                    name: name.clone(),
                                    summary: format_tool_input(&input),
                                });
                                super::render::print_tool_call(&name, &input);
                            }
                            AssistantBlock::Thinking { .. } => {}
                        }
                    }
                }
            }
            RunEventKind::ToolFinished => {
                if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let tool_call = state.pending_tools.remove(&payload.tool_call_id).map(|tc| {
                        ToolCallSummary {
                            name: tc.name,
                            summary: tc.summary,
                        }
                    });
                    print_tool_result(&payload, tool_call.as_ref());
                }
            }
            RunEventKind::AssistantDelta => {
                if let Some(delta) = event.payload.get("delta").and_then(|v| v.as_str()) {
                    terminal_assistant_delta(!state.assistant_prefixed, delta);
                    state.assistant_prefixed = true;
                    state.assistant_open = true;
                    state.streamed_assistant = true;
                }
            }
            RunEventKind::ToolStarted => {}
            RunEventKind::ToolProgress => {}
            RunEventKind::Error => {
                if let Some(message) = event.payload.get("message").and_then(|v| v.as_str()) {
                    finish_assistant_line(&mut state);
                    terminal_writeln(&format!("{RED}error:{RESET} {message}"));
                }
            }
            RunEventKind::RunFinished => {
                if let Some(payload) = payload_as::<RequestFinishedPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let summary = build_run_summary(&payload);
                    if !summary.is_empty() {
                        terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                    }
                }
            }
        }

        Ok(())
    }
}
