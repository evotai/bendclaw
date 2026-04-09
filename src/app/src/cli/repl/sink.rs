use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::markdown::MarkdownStream;
use super::render::count_messages_by_role;
use super::render::format_budget_bar;
use super::render::format_llm_call_lines;
use super::render::format_llm_completed_lines;
use super::render::format_run_summary;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_writeln;
use super::render::ToolCallSummary;
use super::render::DIM;
use super::render::GRAY;
use super::render::GREEN;
use super::render::RED;
use super::render::RESET;
use super::spinner::SpinnerState;
use super::transcript_log::TranscriptLog;
use crate::agent::AssistantBlock;
use crate::agent::RunEvent;
use crate::agent::RunEventPayload;
use crate::session::observability::StatsAggregator;
use crate::types::ContextCompactionCompletedStats;
use crate::types::ContextCompactionStartedStats;
use crate::types::LlmCallCompletedStats;
use crate::types::LlmCallStartedStats;
use crate::types::MessageStats;
use crate::types::ToolFinishedStats;
use crate::types::TranscriptStats;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SinkState {
    pub assistant_open: bool,
    pub streamed_assistant: bool,
    pub pending_tools: HashMap<String, ToolCallDisplay>,
    pub markdown_stream: Option<MarkdownStream>,
    // Unified stats aggregation
    pub aggregator: StatsAggregator,
    // Only needed for real-time path — stats don't cover this
    pub last_message_stats: Option<MessageStats>,
    pub current_model: String,
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
        let mut p = self.user_prompt.lock();
        *p = Some(prompt.to_string());
    }

    fn deactivate_spinner(&self) {
        let mut spinner = self.spinner.lock();
        spinner.clear_if_rendered();
        spinner.deactivate();
    }

    pub fn render(&self, event: &RunEvent) {
        self.write_log(event);
        self.update_spinner(&event.payload);

        let mut state = self.state.lock();
        self.render_output(&event.payload, &mut state);
    }

    fn write_log(&self, event: &RunEvent) {
        let mut log_guard = self.transcript_log.lock();
        if log_guard.is_none() {
            if let Some(log) = TranscriptLog::open(&event.session_id) {
                let mut prompt = self.user_prompt.lock();
                if let Some(p) = prompt.take() {
                    log.write_user_prompt(&p);
                }
                *log_guard = Some(log);
            }
        }
        if let Some(log) = log_guard.as_ref() {
            log.write_event(event);
        }
    }

    fn update_spinner(&self, payload: &RunEventPayload) {
        let mut spinner = self.spinner.lock();
        match payload {
            RunEventPayload::RunStarted {} => spinner.activate(),
            RunEventPayload::TurnStarted {} => spinner.restore_verb(),
            RunEventPayload::AssistantDelta { delta, .. } => {
                if let Some(d) = delta {
                    spinner.add_tokens(d.len() as u64);
                }
            }
            RunEventPayload::AssistantCompleted { .. }
            | RunEventPayload::LlmCallStarted { .. }
            | RunEventPayload::LlmCallCompleted { .. }
            | RunEventPayload::ContextCompactionStarted { .. }
            | RunEventPayload::ContextCompactionCompleted { .. } => {
                spinner.clear_if_rendered();
            }
            RunEventPayload::ToolFinished { .. } => {
                spinner.clear_if_rendered();
                spinner.restore_verb();
            }
            RunEventPayload::ToolStarted { tool_name, .. } => {
                spinner.clear_if_rendered();
                spinner.set_tool(tool_name);
            }
            RunEventPayload::ToolProgress { text, .. } => {
                // Progress lines are rendered by spinner's render_frame.
                spinner.set_progress(text);
            }
            // RunFinished and Error are deferred — spinner stays alive
            // until the output block deactivates it.
            RunEventPayload::RunFinished { .. } | RunEventPayload::Error { .. } => {}
        }
    }

    fn render_output(&self, payload: &RunEventPayload, state: &mut SinkState) {
        match payload {
            RunEventPayload::RunStarted {} => {
                finish_assistant_stream(state);
                state.pending_tools.clear();
                state.aggregator.reset();
                state.last_message_stats = None;
                state.current_model.clear();
            }
            RunEventPayload::TurnStarted {} => {}
            RunEventPayload::AssistantCompleted { content, .. } => {
                for block in content {
                    match block {
                        AssistantBlock::Text { text } => {
                            if state.streamed_assistant {
                                finish_assistant_stream(state);
                            } else if !text.trim().is_empty() {
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
                            finish_assistant_stream(state);
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
                result_tokens,
                duration_ms,
            } => {
                finish_assistant_stream(state);

                // Ingest tool stats via aggregator
                state
                    .aggregator
                    .ingest(&TranscriptStats::ToolFinished(ToolFinishedStats {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        result_tokens: *result_tokens,
                        duration_ms: *duration_ms,
                        is_error: *is_error,
                    }));

                let tool_call =
                    state
                        .pending_tools
                        .remove(tool_call_id)
                        .map(|tc| ToolCallSummary {
                            name: tc.name,
                            summary: tc.summary,
                        });

                if !is_error {
                    if let Some(diff_text) = super::diff::diff_from_details(details) {
                        let title = format!("{tool_name} completed");
                        super::render::print_badge_line(&title, true, true);
                        terminal_writeln(&diff_text);
                        return;
                    }
                }

                print_tool_result(tool_name, content, *is_error, tool_call.as_ref());
            }
            RunEventPayload::AssistantDelta { delta, .. } => {
                if let Some(delta) = delta {
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
                finish_assistant_stream(state);
                state
                    .pending_tools
                    .entry(tool_call_id.clone())
                    .or_insert_with(|| ToolCallDisplay {
                        name: tool_name.clone(),
                        summary: format_tool_input(args),
                    });
                super::render::print_tool_call(tool_name, args, preview_command.as_deref());
            }
            RunEventPayload::ToolProgress { .. } => {
                // Progress rendering is handled entirely by the spinner's render_frame.
            }
            RunEventPayload::Error { message } => {
                finish_assistant_stream(state);
                self.deactivate_spinner();
                terminal_writeln(&format!("{RED}error:{RESET} {message}"));
            }
            RunEventPayload::RunFinished {
                usage,
                turn_count,
                duration_ms,
                ..
            } => {
                finish_assistant_stream(state);
                self.deactivate_spinner();

                let mut data = state
                    .aggregator
                    .to_run_summary(*duration_ms, *turn_count, usage);
                data.last_message_stats = state.last_message_stats.take();

                for line in format_run_summary(&data) {
                    terminal_writeln(&format!("{DIM}{line}{RESET}"));
                }
            }
            RunEventPayload::LlmCallStarted {
                turn,
                attempt,
                model,
                tools,
                messages,
                system_prompt_tokens,
                message_bytes,
                message_count,
                ..
            } => {
                finish_assistant_stream(state);
                state.current_model = model.clone();

                // Ingest via aggregator
                state
                    .aggregator
                    .ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
                        turn: *turn,
                        attempt: *attempt,
                        model: model.clone(),
                        message_count: *message_count,
                        message_bytes: *message_bytes,
                        system_prompt_tokens: *system_prompt_tokens,
                    }));

                let attempt_str = if *attempt > 0 {
                    format!(" · retry {attempt}")
                } else {
                    String::new()
                };
                let title = format!("LLM call · {model} · turn {turn}{attempt_str}");
                super::render::print_badge_line(&title, false, false);
                let stats = count_messages_by_role(messages);
                let detail_lines =
                    format_llm_call_lines(&stats, tools.len(), *system_prompt_tokens);
                for line in &detail_lines {
                    terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
                }
                state.last_message_stats = Some(stats);
                terminal_writeln("");
            }
            RunEventPayload::LlmCallCompleted {
                turn,
                attempt,
                usage,
                error,
                metrics,
                ..
            } => {
                // Ingest via aggregator
                state.aggregator.ingest(&TranscriptStats::LlmCallCompleted(
                    LlmCallCompletedStats {
                        turn: *turn,
                        attempt: *attempt,
                        usage: usage.clone(),
                        metrics: metrics.clone(),
                        error: error.clone(),
                    },
                ));

                let title = if state.current_model.is_empty() {
                    "LLM completed".to_string()
                } else {
                    format!("LLM completed · {}", state.current_model)
                };
                let ok = error.is_none();
                super::render::print_badge_line(&title, true, ok);
                if let Some(err) = error {
                    terminal_writeln(&format!("{RED}  {err}{RESET}"));
                } else {
                    for line in format_llm_completed_lines(usage, metrics.as_ref()) {
                        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
                    }
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
                finish_assistant_stream(state);

                // Ingest via aggregator
                state
                    .aggregator
                    .ingest(&TranscriptStats::ContextCompactionStarted(
                        ContextCompactionStartedStats {
                            message_count: *message_count,
                            estimated_tokens: *estimated_tokens,
                            budget_tokens: *budget_tokens,
                            system_prompt_tokens: *system_prompt_tokens,
                            context_window: *context_window,
                        },
                    ));

                let title = "compact call";
                super::render::print_badge_line(title, false, false);
                let h_est = super::render::human_tokens(*estimated_tokens);
                terminal_writeln(&format!(
                    "{GRAY}  {message_count} messages · ~{h_est} tokens{RESET}",
                ));
                let bar = format_budget_bar(*estimated_tokens, *budget_tokens, 40);
                terminal_writeln(&format!("{GRAY}  {bar} of budget{RESET}"));
                let h_budget = super::render::human_tokens(*budget_tokens);
                let h_window = super::render::human_tokens(*context_window);
                let h_sys = super::render::human_tokens(*system_prompt_tokens);
                terminal_writeln(&format!(
                    "{GRAY}  budget {h_budget} (window {h_window} − sys {h_sys}){RESET}",
                ));
                terminal_writeln("");
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
                before_tool_details: _,
                after_tool_details: _,
                actions,
            } => {
                // Ingest via aggregator
                state
                    .aggregator
                    .ingest(&TranscriptStats::ContextCompactionCompleted(
                        ContextCompactionCompletedStats {
                            level: *level,
                            before_message_count: *before_message_count,
                            after_message_count: *after_message_count,
                            before_estimated_tokens: *before_estimated_tokens,
                            after_estimated_tokens: *after_estimated_tokens,
                            tool_outputs_truncated: *tool_outputs_truncated,
                            turns_summarized: *turns_summarized,
                            messages_dropped: *messages_dropped,
                            actions: actions
                                .iter()
                                .map(|a| crate::types::CompactionActionStats {
                                    index: a.index,
                                    tool_name: a.tool_name.clone(),
                                    method: a.method.clone(),
                                    before_tokens: a.before_tokens,
                                    after_tokens: a.after_tokens,
                                    end_index: a.end_index,
                                    related_count: a.related_count,
                                })
                                .collect(),
                        },
                    ));

                if *level > 0 {
                    let saved = before_estimated_tokens.saturating_sub(*after_estimated_tokens);
                    let saved_pct = if *before_estimated_tokens > 0 {
                        saved as f64 / *before_estimated_tokens as f64 * 100.0
                    } else {
                        0.0
                    };
                    let h_saved = super::render::human_tokens(saved);

                    // Build summary line based on level
                    let mut summary_parts = Vec::new();
                    if *tool_outputs_truncated > 0 {
                        summary_parts
                            .push(format!("truncated {tool_outputs_truncated} tool outputs"));
                    }
                    if *turns_summarized > 0 {
                        summary_parts.push(format!("summarized {turns_summarized} turns"));
                    }
                    if *messages_dropped > 0 {
                        summary_parts.push(format!("dropped {messages_dropped} messages"));
                    }
                    let summary_suffix = if summary_parts.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", summary_parts.join(" · "))
                    };

                    let title = format!("compact completed · level {level}");
                    super::render::print_badge_line(&title, true, true);
                    terminal_writeln(&format!(
                        "{GREEN}  saved ~{h_saved} tokens ({saved_pct:.1}%){summary_suffix}{RESET}",
                    ));
                    let h_before = super::render::human_tokens(*before_estimated_tokens);
                    let h_after = super::render::human_tokens(*after_estimated_tokens);
                    terminal_writeln(&format!(
                        "{GRAY}  {before_message_count} messages ~{h_before} tok → {after_message_count} messages ~{h_after} tok{RESET}",
                    ));

                    // Per-action detail (only non-Skipped actions)
                    for a in actions {
                        let h_before = super::render::human_tokens(a.before_tokens);
                        let h_after = super::render::human_tokens(a.after_tokens);
                        match a.method.as_str() {
                            "Summarized" => {
                                let rc = a.related_count.unwrap_or(0);
                                terminal_writeln(&format!(
                                    "{GRAY}  #{:<3} assistant(+{} results) {:<12} ~{} → ~{}{RESET}",
                                    a.index, rc, a.method, h_before, h_after,
                                ));
                            }
                            "Dropped" => {
                                if let Some(end) = a.end_index {
                                    terminal_writeln(&format!(
                                        "{GRAY}  #{}..#{} {:<12} {:<12} ~{} → ~{}{RESET}",
                                        a.index, end, "messages", a.method, h_before, h_after,
                                    ));
                                } else {
                                    let rc = a.related_count.unwrap_or(0);
                                    terminal_writeln(&format!(
                                        "{GRAY}  Dropped {} messages ~{} → ~{}{RESET}",
                                        rc, h_before, h_after,
                                    ));
                                }
                            }
                            _ => {
                                // Level 1: Outline / HeadTail
                                terminal_writeln(&format!(
                                    "{GRAY}  #{:<3} {:<12} {:<12} ~{} → ~{}{RESET}",
                                    a.index, a.tool_name, a.method, h_before, h_after,
                                ));
                            }
                        }
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

impl Default for ReplSink {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(SpinnerState::new())))
    }
}
