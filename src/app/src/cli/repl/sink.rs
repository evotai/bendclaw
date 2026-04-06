use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use super::markdown::MarkdownStream;
use super::render::build_run_summary;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_writeln;
use super::render::ToolCallSummary;
use super::render::DIM;
use super::render::GRAY;
use super::render::RED;
use super::render::RESET;
use super::spinner::SpinnerState;
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
    pub streamed_assistant: bool,
    pub pending_tools: HashMap<String, ToolCallDisplay>,
    pub markdown_stream: Option<MarkdownStream>,
    pub llm_call_count: u32,
    pub tool_call_count: u32,
}

pub struct ToolCallDisplay {
    pub name: String,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn finish_assistant_stream(state: &mut SinkState) {
    if let Some(stream) = state.markdown_stream.take() {
        let _ = stream.finish();
    }
    if state.assistant_open {
        terminal_writeln("");
    }
    state.assistant_open = false;
    state.streamed_assistant = false;
}

// ---------------------------------------------------------------------------
// ReplSink
// ---------------------------------------------------------------------------

pub struct ReplSink {
    state: Mutex<SinkState>,
    spinner: Arc<Mutex<SpinnerState>>,
}

impl ReplSink {
    pub fn new(spinner: Arc<Mutex<SpinnerState>>) -> Self {
        Self {
            state: Mutex::new(SinkState::default()),
            spinner,
        }
    }
}

impl Default for ReplSink {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(SpinnerState::new())))
    }
}

#[async_trait]
impl EventSink for ReplSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        // Lock spinner, update state and clear line before any output
        {
            let mut spinner = self
                .spinner
                .lock()
                .map_err(|_| crate::error::BendclawError::Cli("spinner lock poisoned".into()))?;

            match &event.payload {
                RunEventPayload::RunStarted {} => {
                    spinner.activate();
                }
                RunEventPayload::TurnStarted {} => {
                    spinner.restore_verb();
                }
                RunEventPayload::AssistantDelta { delta, .. } => {
                    if let Some(d) = delta {
                        spinner.add_tokens(d.len() as u64);
                    }
                    spinner.clear_if_rendered();
                    spinner.hide();
                }
                RunEventPayload::AssistantCompleted { .. } => {
                    spinner.clear_if_rendered();
                    spinner.hide();
                }
                RunEventPayload::ToolStarted { tool_name, .. } => {
                    spinner.clear_if_rendered();
                    spinner.set_tool(tool_name);
                }
                RunEventPayload::ToolProgress { text, .. } => {
                    spinner.set_progress(text);
                }
                RunEventPayload::ToolFinished { .. } => {
                    spinner.clear_if_rendered();
                    spinner.hide();
                }
                RunEventPayload::RunFinished { .. } => {
                    spinner.clear_if_rendered();
                    spinner.deactivate();
                }
                RunEventPayload::Error { .. } => {
                    spinner.clear_if_rendered();
                    spinner.deactivate();
                }
                RunEventPayload::LlmCallStarted { .. } => {}
                RunEventPayload::LlmCallCompleted { .. } => {}
            }
        };

        let mut state = self
            .state
            .lock()
            .map_err(|_| crate::error::BendclawError::Cli("sink state lock poisoned".into()))?;

        match &event.payload {
            RunEventPayload::RunStarted {} => {
                finish_assistant_stream(&mut state);
                state.pending_tools.clear();
                state.llm_call_count = 0;
                state.tool_call_count = 0;
            }
            RunEventPayload::TurnStarted {} => {}
            RunEventPayload::AssistantCompleted { content, .. } => {
                for block in content {
                    match block {
                        AssistantBlock::Text { text } => {
                            if state.streamed_assistant {
                                // Stream already rendered the content; just close it
                                finish_assistant_stream(&mut state);
                            } else if !text.trim().is_empty() {
                                // Non-streamed: render the full text through markdown
                                let mut stream = MarkdownStream::new(self.spinner.clone());
                                let _ = stream.push(text);
                                let _ = stream.push("\n");
                                let _ = stream.finish();
                                terminal_writeln("");
                            }
                            state.assistant_open = false;
                            state.streamed_assistant = false;
                        }
                        AssistantBlock::ToolCall { id, name, input } => {
                            finish_assistant_stream(&mut state);
                            state.pending_tools.insert(id.clone(), ToolCallDisplay {
                                name: name.clone(),
                                summary: format_tool_input(input),
                            });
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
                finish_assistant_stream(&mut state);
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
                    // Lazily create the markdown stream on first delta
                    if state.markdown_stream.is_none() {
                        state.markdown_stream = Some(MarkdownStream::new(self.spinner.clone()));
                    }
                    if let Some(ref mut stream) = state.markdown_stream {
                        let _ = stream.push(delta);
                    }
                    state.assistant_open = true;
                    state.streamed_assistant = true;
                }
            }
            RunEventPayload::ToolStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                finish_assistant_stream(&mut state);
                state.tool_call_count += 1;
                state
                    .pending_tools
                    .entry(tool_call_id.clone())
                    .or_insert_with(|| ToolCallDisplay {
                        name: tool_name.clone(),
                        summary: format_tool_input(args),
                    });
                super::render::print_tool_call(tool_name, args);
            }
            RunEventPayload::ToolProgress { .. } => {}
            RunEventPayload::Error { message } => {
                finish_assistant_stream(&mut state);
                terminal_writeln(&format!("{RED}error:{RESET} {message}"));
            }
            RunEventPayload::RunFinished {
                usage,
                turn_count,
                duration_ms,
                ..
            } => {
                finish_assistant_stream(&mut state);
                let summary = build_run_summary(
                    usage,
                    *turn_count,
                    *duration_ms,
                    state.llm_call_count,
                    state.tool_call_count,
                );
                if !summary.is_empty() {
                    terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                }
            }
            RunEventPayload::LlmCallStarted {
                turn,
                model,
                messages,
                tools,
                message_bytes,
                ..
            } => {
                finish_assistant_stream(&mut state);
                state.llm_call_count += 1;
                let title = format!("LLM call · {model} · turn {turn}");
                super::render::print_badge_line(&title, false, false);
                terminal_writeln(&format!(
                    "{GRAY}  {} messages · {} tools · {} bytes{RESET}",
                    messages.len(),
                    tools.len(),
                    message_bytes,
                ));
                terminal_writeln("");
            }
            RunEventPayload::LlmCallCompleted { usage, error, .. } => {
                let title = "LLM completed".to_string();
                let ok = error.is_none();
                super::render::print_badge_line(&title, true, ok);
                if let Some(err) = error {
                    terminal_writeln(&format!("{RED}  {err}{RESET}"));
                } else {
                    terminal_writeln(&format!(
                        "{GRAY}  {} input · {} output tokens{RESET}",
                        usage.input, usage.output,
                    ));
                }
                terminal_writeln("");
            }
        }

        Ok(())
    }
}
