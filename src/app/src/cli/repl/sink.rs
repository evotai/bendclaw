use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::markdown::MarkdownStream;
use super::render::count_messages_by_role;
use super::render::format_budget_bar;
use super::render::format_llm_call_lines;
use super::render::format_llm_completed_lines;
use super::render::format_run_summary;
use super::render::format_tool_breakdown;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_writeln;
use super::render::CompactRecord;
use super::render::RunSummaryData;
use super::render::ToolAggStats;
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
    // Run summary aggregation
    pub system_prompt_tokens: usize,
    pub last_message_stats: Option<super::render::MessageStats>,
    pub llm_metrics: Vec<crate::agent::LlmCallMetrics>,
    pub llm_output_tokens: Vec<u64>,
    pub tool_stats: HashMap<String, ToolAggStats>,
    pub compact_history: Vec<CompactRecord>,
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
            RunEventPayload::ToolProgress { text, .. } => spinner.set_progress(text),
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
                state.llm_call_count = 0;
                state.tool_call_count = 0;
                state.system_prompt_tokens = 0;
                state.last_message_stats = None;
                state.llm_metrics.clear();
                state.llm_output_tokens.clear();
                state.tool_stats.clear();
                state.compact_history.clear();
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

                // Accumulate tool stats for run summary
                let entry = state.tool_stats.entry(tool_name.clone()).or_default();
                entry.calls += 1;
                entry.result_tokens += result_tokens;
                entry.duration_ms += duration_ms;
                if *is_error {
                    entry.errors += 1;
                }

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

                // Collect tool stats sorted by result_tokens desc
                let mut tool_stats: Vec<(String, ToolAggStats)> =
                    state.tool_stats.drain().collect();
                tool_stats.sort_by(|a, b| b.1.result_tokens.cmp(&a.1.result_tokens));

                let data = RunSummaryData {
                    duration_ms: *duration_ms,
                    turn_count: *turn_count,
                    usage: usage.clone(),
                    llm_call_count: state.llm_call_count,
                    tool_call_count: state.tool_call_count,
                    system_prompt_tokens: state.system_prompt_tokens,
                    last_message_stats: state.last_message_stats.take(),
                    llm_metrics: std::mem::take(&mut state.llm_metrics),
                    llm_output_tokens: std::mem::take(&mut state.llm_output_tokens),
                    tool_stats,
                    compact_history: std::mem::take(&mut state.compact_history),
                };

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
                ..
            } => {
                finish_assistant_stream(state);
                state.llm_call_count += 1;
                state.system_prompt_tokens = *system_prompt_tokens;
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
                usage,
                error,
                metrics,
                ..
            } => {
                let title = "LLM completed".to_string();
                let ok = error.is_none();
                super::render::print_badge_line(&title, true, ok);
                if let Some(err) = error {
                    terminal_writeln(&format!("{RED}  {err}{RESET}"));
                } else {
                    for line in format_llm_completed_lines(usage, metrics.as_ref()) {
                        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
                    }
                }
                if let Some(m) = metrics {
                    state.llm_metrics.push(m.clone());
                }
                state.llm_output_tokens.push(usage.output);
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
                let title = "compact call";
                super::render::print_badge_line(title, false, false);
                terminal_writeln(&format!(
                    "{GRAY}  {message_count} messages · ~{estimated_tokens} tokens{RESET}",
                ));
                let bar = format_budget_bar(*estimated_tokens, *budget_tokens, 40);
                terminal_writeln(&format!("{GRAY}  {bar} of budget{RESET}"));
                terminal_writeln(&format!(
                    "{GRAY}  budget {budget_tokens} (window {context_window} − sys {system_prompt_tokens}){RESET}",
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
                before_tool_details,
                after_tool_details,
            } => {
                // Accumulate compact history for run summary
                state.compact_history.push(CompactRecord {
                    level: *level,
                    before_tokens: *before_estimated_tokens,
                    after_tokens: *after_estimated_tokens,
                });

                if *level > 0 {
                    let saved = before_estimated_tokens.saturating_sub(*after_estimated_tokens);
                    let saved_pct = if *before_estimated_tokens > 0 {
                        saved as f64 / *before_estimated_tokens as f64 * 100.0
                    } else {
                        0.0
                    };
                    let removed = before_message_count.saturating_sub(*after_message_count);
                    let title = format!("compact completed · level {level}");
                    super::render::print_badge_line(&title, true, true);
                    terminal_writeln(&format!(
                        "{GREEN}  saved ~{saved} tokens ({saved_pct:.1}%) · {removed} messages removed{RESET}",
                    ));
                    terminal_writeln(&format!(
                        "{GRAY}  {before_message_count} messages ~{before_estimated_tokens} tok → {after_message_count} messages ~{after_estimated_tokens} tok{RESET}",
                    ));

                    // Per-tool before/after breakdown
                    if !before_tool_details.is_empty() {
                        terminal_writeln(&format!("{GRAY}  tool results before:{RESET}"));
                        let before_total: usize = before_tool_details.iter().map(|(_, t)| t).sum();
                        for line in format_tool_breakdown(before_tool_details, before_total) {
                            terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
                        }
                    }
                    if !after_tool_details.is_empty() {
                        terminal_writeln(&format!("{GRAY}  tool results after:{RESET}"));
                        let after_total: usize = after_tool_details.iter().map(|(_, t)| t).sum();
                        for line in format_tool_breakdown(after_tool_details, after_total) {
                            terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
                        }
                    }

                    let mut actions = Vec::new();
                    if *tool_outputs_truncated > 0 {
                        actions.push(format!("truncated {tool_outputs_truncated} tools"));
                    }
                    if *turns_summarized > 0 {
                        actions.push(format!("summarized {turns_summarized} turns"));
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

impl Default for ReplSink {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(SpinnerState::new())))
    }
}
