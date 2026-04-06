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
use crate::protocol::AssistantBlock;
use crate::protocol::RunEvent;
use crate::protocol::RunEventPayload;
use crate::request::EventSink;

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

        match &event.payload {
            RunEventPayload::RunStarted {} => {
                state.assistant_open = false;
                state.assistant_prefixed = false;
                state.streamed_assistant = false;
                state.pending_tools.clear();
            }
            RunEventPayload::TurnStarted {} => {}
            RunEventPayload::AssistantCompleted { content, .. } => {
                for block in content {
                    match block {
                        AssistantBlock::Text { text } => {
                            if state.streamed_assistant {
                                terminal_writeln("");
                            } else if !text.trim().is_empty() {
                                terminal_prefixed_writeln(text);
                            }
                            state.assistant_open = false;
                            state.assistant_prefixed = false;
                            state.streamed_assistant = false;
                        }
                        AssistantBlock::ToolCall { id, name, input } => {
                            finish_assistant_line(&mut state);
                            state.pending_tools.insert(id.clone(), ToolCallDisplay {
                                name: name.clone(),
                                summary: format_tool_input(input),
                            });
                            super::render::print_tool_call(name, input);
                        }
                        AssistantBlock::Thinking { .. } => {}
                    }
                }
            }
            RunEventPayload::ToolFinished {
                tool_call_id,
                tool_name,
                content,
                is_error,
            } => {
                finish_assistant_line(&mut state);
                let tool_call =
                    state
                        .pending_tools
                        .remove(tool_call_id)
                        .map(|tc| ToolCallSummary {
                            name: tc.name,
                            summary: tc.summary,
                        });
                print_tool_result(tool_name, content, *is_error, tool_call.as_ref());
            }
            RunEventPayload::AssistantDelta { delta, .. } => {
                if let Some(delta) = delta {
                    terminal_assistant_delta(!state.assistant_prefixed, delta);
                    state.assistant_prefixed = true;
                    state.assistant_open = true;
                    state.streamed_assistant = true;
                }
            }
            RunEventPayload::ToolStarted { .. } => {}
            RunEventPayload::ToolProgress { .. } => {}
            RunEventPayload::Error { message } => {
                finish_assistant_line(&mut state);
                terminal_writeln(&format!("{RED}error:{RESET} {message}"));
            }
            RunEventPayload::RunFinished {
                usage,
                turn_count,
                duration_ms,
                ..
            } => {
                finish_assistant_line(&mut state);
                let summary = build_run_summary(usage, *turn_count, *duration_ms);
                if !summary.is_empty() {
                    terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                }
            }
        }

        Ok(())
    }
}
