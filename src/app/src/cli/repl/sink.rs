use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use super::markdown::MarkdownStream;
use super::render::build_run_summary;
use super::render::count_messages_by_role;
use super::render::format_llm_call_lines;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_writeln;
use super::render::ToolCallSummary;
use super::render::DIM;
use super::render::GRAY;
use super::render::RED;
use super::render::RESET;
use super::spinner::SpinnerState;
use super::transcript_log::TranscriptLog;
use crate::protocol::AssistantBlock;
use crate::protocol::RunEvent;
use crate::protocol::RunEventPayload;

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

fn format_cache_info(cache_read: u64, cache_write: u64, input: u64) -> String {
    if cache_read == 0 && cache_write == 0 {
        return String::new();
    }
    let total_input = input + cache_read + cache_write;
    let hit_rate = if total_input > 0 {
        cache_read as f64 / total_input as f64 * 100.0
    } else {
        0.0
    };
    format!(" · cache r:{cache_read} w:{cache_write} hit:{hit_rate:.0}%")
}

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
    transcript_log: Mutex<Option<TranscriptLog>>,
    user_prompt: Mutex<Option<String>>,
}

impl ReplSink {
    pub fn new(spinner: Arc<Mutex<SpinnerState>>) -> Self {
        Self {
            state: Mutex::new(SinkState::default()),
            spinner,
            transcript_log: Mutex::new(None),
            user_prompt: Mutex::new(None),
        }
    }

    pub fn set_user_prompt(&self, prompt: &str) {
        if let Ok(mut p) = self.user_prompt.lock() {
            *p = Some(prompt.to_string());
        }
    }

    fn deactivate_spinner(&self) {
        if let Ok(mut spinner) = self.spinner.lock() {
            spinner.clear_if_rendered();
            spinner.deactivate();
        }
    }
}

impl Default for ReplSink {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(SpinnerState::new())))
    }
}

impl ReplSink {
    pub fn render(&self, event: &RunEvent) {
        // Lazily open transcript log on first event (session_id is now known)
        if let Ok(mut log_guard) = self.transcript_log.lock() {
            if log_guard.is_none() {
                if let Some(log) = TranscriptLog::open(&event.session_id) {
                    // Write the user prompt that was buffered before the session started
                    if let Ok(mut prompt) = self.user_prompt.lock() {
                        if let Some(p) = prompt.take() {
                            log.write_user_prompt(&p);
                        }
                    }
                    *log_guard = Some(log);
                }
            }
            if let Some(log) = log_guard.as_ref() {
                log.write_event(event);
            }
        }

        // Lock spinner, update state and clear line before any output
        {
            let Ok(mut spinner) = self.spinner.lock() else {
                return;
            };

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
                }
                RunEventPayload::AssistantCompleted { .. } => {
                    spinner.clear_if_rendered();
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
                    spinner.restore_verb();
                }
                RunEventPayload::RunFinished { .. } => {
                    // Deferred to output block below — keep spinner alive
                    // until the summary line is ready to print.
                }
                RunEventPayload::Error { .. } => {
                    // Deferred to output block below.
                }
                RunEventPayload::LlmCallStarted { .. } => {
                    spinner.clear_if_rendered();
                }
                RunEventPayload::LlmCallCompleted { .. } => {
                    spinner.clear_if_rendered();
                }
                RunEventPayload::ContextCompactionStarted { .. } => {
                    spinner.clear_if_rendered();
                }
                RunEventPayload::ContextCompactionCompleted { .. } => {
                    spinner.clear_if_rendered();
                }
            }
        };

        let Ok(mut state) = self.state.lock() else {
            return;
        };

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
                details,
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

                // Show diff for file-modifying tools, fall back to normal result
                if !is_error {
                    if let Some(diff_text) = super::diff::diff_from_details(details) {
                        let title = if *is_error {
                            format!("{tool_name} failed")
                        } else {
                            format!("{tool_name} completed")
                        };
                        super::render::print_badge_line(&title, true, true);
                        terminal_writeln(&diff_text);
                        return;
                    }
                }

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
                preview_command,
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
                super::render::print_tool_call(tool_name, args, preview_command.as_deref());
            }
            RunEventPayload::ToolProgress { .. } => {}
            RunEventPayload::Error { message } => {
                finish_assistant_stream(&mut state);
                self.deactivate_spinner();
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
                self.deactivate_spinner();
                if !summary.is_empty() {
                    terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                }
            }
            RunEventPayload::LlmCallStarted {
                turn,
                attempt,
                model,
                tools,
                messages,
                system_prompt_tokens,
                ..
            } => {
                finish_assistant_stream(&mut state);
                state.llm_call_count += 1;
                let attempt_str = if *attempt > 0 {
                    format!(" · retry {attempt}")
                } else {
                    String::new()
                };
                let title = format!("LLM call · {model} · turn {turn}{attempt_str}");
                super::render::print_badge_line(&title, false, false);
                let stats = count_messages_by_role(messages);
                let (msg_line, token_line) =
                    format_llm_call_lines(&stats, tools.len(), *system_prompt_tokens);
                terminal_writeln(&format!("{GRAY}  {msg_line}{RESET}"));
                terminal_writeln(&format!("{GRAY}  {token_line}{RESET}"));
                terminal_writeln("");
            }
            RunEventPayload::LlmCallCompleted {
                usage,
                cache_read,
                cache_write,
                error,
                ..
            } => {
                let title = "LLM completed".to_string();
                let ok = error.is_none();
                super::render::print_badge_line(&title, true, ok);
                if let Some(err) = error {
                    terminal_writeln(&format!("{RED}  {err}{RESET}"));
                } else {
                    let cache_str = format_cache_info(*cache_read, *cache_write, usage.input);
                    terminal_writeln(&format!(
                        "{GRAY}  {} input · {} output tokens{cache_str}{RESET}",
                        usage.input, usage.output,
                    ));
                }
                terminal_writeln("");
            }
            RunEventPayload::ContextCompactionStarted {
                message_count,
                estimated_tokens,
                budget_tokens,
                system_prompt_tokens,
                context_window,
            } => {
                finish_assistant_stream(&mut state);
                let usage_pct = if *budget_tokens > 0 {
                    *estimated_tokens as f64 / *budget_tokens as f64 * 100.0
                } else {
                    0.0
                };
                let title = format!("compact · {message_count} messages · ~{estimated_tokens} tokens · {usage_pct:.0}% of budget");
                super::render::print_badge_line(&title, false, false);
                terminal_writeln(&format!(
                    "{GRAY}  budget: {budget_tokens} (window {context_window} − sys {system_prompt_tokens}){RESET}",
                ));
            }
            RunEventPayload::ContextCompactionCompleted {
                level,
                before_message_count,
                after_message_count,
                before_estimated_tokens,
                after_estimated_tokens,
                tool_outputs_truncated,
                turns_summarized,
                messages_dropped,
            } => {
                if *level > 0 {
                    let saved = before_estimated_tokens.saturating_sub(*after_estimated_tokens);
                    let removed = before_message_count.saturating_sub(*after_message_count);
                    let title = format!("compact completed · level {level}");
                    super::render::print_badge_line(&title, true, true);
                    terminal_writeln(&format!(
                        "{GRAY}  {after_message_count} messages · ~{after_estimated_tokens} tokens · saved ~{saved} tokens · {removed} messages removed{RESET}",
                    ));
                    let mut actions = Vec::new();
                    if *tool_outputs_truncated > 0 {
                        actions.push(format!("truncated {tool_outputs_truncated}"));
                    }
                    if *turns_summarized > 0 {
                        actions.push(format!("summarized {turns_summarized}"));
                    }
                    if *messages_dropped > 0 {
                        actions.push(format!("dropped {messages_dropped}"));
                    }
                    if !actions.is_empty() {
                        terminal_writeln(&format!(
                            "{GRAY}  actions: {}{RESET}",
                            actions.join(" · "),
                        ));
                    }
                } else {
                    let title = "compact completed · level 0".to_string();
                    super::render::print_badge_line(&title, true, true);
                    terminal_writeln(&format!("{GRAY}  no compaction needed{RESET}"));
                }
                terminal_writeln("");
            }
        }
    }
}
