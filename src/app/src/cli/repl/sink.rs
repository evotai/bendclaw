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
use crate::cli::format::mask_secrets;
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
    pub aggregator: StatsAggregator,
    pub last_message_stats: Option<MessageStats>,
    pub current_model: String,
    pub pending_budget: Option<PendingBudget>,
}

pub struct PendingBudget {
    pub message_count: usize,
    pub estimated_tokens: usize,
    pub budget_tokens: usize,
    pub system_prompt_tokens: usize,
    pub context_window: usize,
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

fn render_compact_started(budget: &PendingBudget) {
    let title = "compact call";
    super::render::print_badge_line(title, false, false);
    let h_est = super::render::human_tokens(budget.estimated_tokens);
    terminal_writeln(&format!(
        "{GRAY}  {} messages · ~{h_est} tokens{RESET}",
        budget.message_count,
    ));
    let bar = format_budget_bar(budget.estimated_tokens, budget.budget_tokens, 40);
    terminal_writeln(&format!("{GRAY}  {bar}{RESET}"));
    let h_budget = super::render::human_tokens(budget.budget_tokens);
    let h_window = super::render::human_tokens(budget.context_window);
    let h_sys = super::render::human_tokens(budget.system_prompt_tokens);
    terminal_writeln(&format!(
        "{GRAY}  budget {h_budget} (window {h_window} − sys {h_sys}){RESET}",
    ));
    terminal_writeln("");
}

// ---------------------------------------------------------------------------
// ReplSink
// ---------------------------------------------------------------------------

pub struct ReplSink {
    state: Mutex<SinkState>,
    spinner: Arc<Mutex<SpinnerState>>,
    transcript_log: Mutex<Option<TranscriptLog>>,
    user_prompt: Mutex<Option<String>>,
    /// Secret variable values for display-layer masking.
    secret_values: Mutex<Vec<String>>,
}

impl ReplSink {
    pub fn new(spinner: Arc<Mutex<SpinnerState>>) -> Self {
        Self {
            state: Mutex::new(SinkState::default()),
            spinner,
            transcript_log: Mutex::new(None),
            user_prompt: Mutex::new(None),
            secret_values: Mutex::new(Vec::new()),
        }
    }

    pub fn set_user_prompt(&self, prompt: &str) {
        let mut p = self.user_prompt.lock();
        *p = Some(prompt.to_string());
    }

    /// Update the set of secret values used for display masking.
    pub fn set_secret_values(&self, values: Vec<String>) {
        *self.secret_values.lock() = values;
    }

    /// Replace all known secret values in `text` with their masked form.
    fn mask_secrets_text(&self, text: &str) -> String {
        let secrets = self.secret_values.lock();
        mask_secrets(text, &secrets)
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
                let masked = self.mask_secrets_text(text);
                spinner.set_progress(&masked);
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
                state.pending_budget = None;
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

                let masked_content = self.mask_secrets_text(content);
                print_tool_result(tool_name, &masked_content, *is_error, tool_call.as_ref());
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

                state.pending_budget = Some(PendingBudget {
                    message_count: *message_count,
                    estimated_tokens: *estimated_tokens,
                    budget_tokens: *budget_tokens,
                    system_prompt_tokens: *system_prompt_tokens,
                    context_window: *context_window,
                });
            }
            RunEventPayload::ContextCompactionCompleted { result } => {
                // Ingest directly — result is already the unified type
                state
                    .aggregator
                    .ingest(&TranscriptStats::ContextCompactionCompleted(
                        ContextCompactionCompletedStats {
                            result: result.clone(),
                        },
                    ));

                // Only render compact started + completed for real compactions
                let is_noop = matches!(result, crate::types::CompactionResult::NoOp);
                if !is_noop {
                    if let Some(budget) = state.pending_budget.take() {
                        render_compact_started(&budget);
                    }
                } else {
                    state.pending_budget = None;
                }

                match result {
                    crate::types::CompactionResult::LevelCompacted {
                        level,
                        before_message_count,
                        after_message_count,
                        before_estimated_tokens,
                        after_estimated_tokens,
                        messages_dropped,
                        actions,
                        ..
                    } => {
                        let saved = before_estimated_tokens.saturating_sub(*after_estimated_tokens);
                        let saved_pct = if *before_estimated_tokens > 0 {
                            saved as f64 / *before_estimated_tokens as f64 * 100.0
                        } else {
                            0.0
                        };
                        let h_saved = super::render::human_tokens(saved);
                        let h_before = super::render::human_tokens(*before_estimated_tokens);
                        let h_after = super::render::human_tokens(*after_estimated_tokens);

                        let mut sorted: Vec<_> =
                            actions.iter().filter(|a| a.method != "Skipped").collect();
                        sorted.sort_by(|a, b| {
                            let sa = a.before_tokens.saturating_sub(a.after_tokens);
                            let sb = b.before_tokens.saturating_sub(b.after_tokens);
                            sb.cmp(&sa)
                        });

                        let title = format!("compact · L{level}");
                        super::render::print_badge_line(&title, true, true);

                        terminal_writeln(&format!(
                            "{GRAY}  {before_message_count} messages ~{h_before} tok{RESET}",
                        ));

                        let bar = render_position_bar(*before_message_count, &sorted, *level);
                        terminal_writeln(&format!("{GRAY}  {bar}{RESET}"));

                        let summary = format_action_summary(
                            *level,
                            &sorted,
                            *messages_dropped,
                            *after_message_count,
                        );
                        terminal_writeln(&format!("{GRAY}  {summary}{RESET}"));

                        terminal_writeln(&format!(
                            "{GREEN}  {after_message_count} messages ~{h_after} tok  (saved ~{h_saved}, {saved_pct:.1}%){RESET}",
                        ));

                        if !sorted.is_empty() {
                            let total_actions = actions.len();
                            let changed = sorted.len();

                            let header = match *level {
                                1 => format!(
                                    "  actions: ({changed} of {total_actions} changed, sorted by savings)"
                                ),
                                2 => {
                                    let total_msgs: usize = sorted
                                        .iter()
                                        .map(|a| 1 + a.related_count.unwrap_or(0))
                                        .sum();
                                    format!(
                                        "  actions: ({changed} turns, {total_msgs} msgs → {changed} summaries)"
                                    )
                                }
                                3 => {
                                    let kept = after_message_count.saturating_sub(1);
                                    format!(
                                        "  actions: ({} dropped, {} kept, 1 marker)",
                                        messages_dropped, kept
                                    )
                                }
                                _ => format!("  actions: ({changed} changed)"),
                            };
                            terminal_writeln(&format!("{GRAY}{header}{RESET}"));

                            const TOP: usize = 3;
                            const TAIL: usize = 2;

                            let render_action = |a: &&crate::types::CompactionAction| {
                                let hb = super::render::human_tokens(a.before_tokens);
                                let ha = super::render::human_tokens(a.after_tokens);
                                let saved_tok = a.before_tokens.saturating_sub(a.after_tokens);
                                let hs = super::render::human_tokens(saved_tok);
                                match a.method.as_str() {
                                    "Summarized" => {
                                        let rc = a.related_count.unwrap_or(0);
                                        terminal_writeln(&format!(
                                            "{GRAY}    #{:<3} turn({} msgs)  {:<12} ~{} → ~{}  (saved ~{}){RESET}",
                                            a.index, 1 + rc, a.method, hb, ha, hs,
                                        ));
                                    }
                                    "Dropped" => {
                                        if let Some(end) = a.end_index {
                                            terminal_writeln(&format!(
                                                "{GRAY}    #{}..#{:<3} {:<12} ~{} → ~{}  (saved ~{}){RESET}",
                                                a.index, end, a.method, hb, ha, hs,
                                            ));
                                        } else {
                                            terminal_writeln(&format!(
                                                "{GRAY}    #{:<3} {:<12} ~{} → ~{}  (saved ~{}){RESET}",
                                                a.index, a.method, hb, ha, hs,
                                            ));
                                        }
                                    }
                                    _ => {
                                        terminal_writeln(&format!(
                                            "{GRAY}    #{:<3} {:<12} {:<12} ~{} → ~{}  (saved ~{}){RESET}",
                                            a.index, a.tool_name, a.method, hb, ha, hs,
                                        ));
                                    }
                                }
                            };

                            if sorted.len() <= TOP + TAIL {
                                for a in &sorted {
                                    render_action(a);
                                }
                            } else {
                                for a in &sorted[..TOP] {
                                    render_action(a);
                                }
                                let omitted = sorted.len() - TOP - TAIL;
                                terminal_writeln(&format!(
                                    "{GRAY}    ... {omitted} more ...{RESET}"
                                ));
                                for a in &sorted[sorted.len() - TAIL..] {
                                    render_action(a);
                                }
                            }
                        }
                    }
                    crate::types::CompactionResult::RunOnceCleared {
                        cleared_count,
                        saved_tokens,
                        ..
                    } => {
                        let title = "compact · run-once";
                        super::render::print_badge_line(title, true, true);
                        let h_saved = super::render::human_tokens(*saved_tokens);
                        terminal_writeln(&format!(
                            "{GRAY}  cleared {cleared_count} run-once tool result(s) · saved ~{h_saved}{RESET}"
                        ));
                    }
                    crate::types::CompactionResult::NoOp => {}
                }
                terminal_writeln("");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Compact display helpers
// ---------------------------------------------------------------------------

fn render_position_bar(
    before_count: usize,
    sorted_actions: &[&crate::types::CompactionAction],
    level: u8,
) -> String {
    const WIDTH: usize = 40;
    if before_count == 0 {
        return format!("[{}]", "·".repeat(WIDTH));
    }

    let default_char = if level == 3 { 'K' } else { '·' };
    let mut slots = vec![default_char; WIDTH.min(before_count)];
    let slot_count = slots.len();

    for a in sorted_actions {
        let start = a.index;
        let end = a.end_index.unwrap_or(a.index);
        let ch = match (level, a.method.as_str()) {
            (1, "Outline") => 'O',
            (1, "HeadTail") => 'H',
            (2, "Summarized") => 'S',
            (3, "Dropped") => 'D',
            _ => '?',
        };

        if before_count <= WIDTH {
            for slot in slots
                .iter_mut()
                .take(end.min(slot_count.saturating_sub(1)) + 1)
                .skip(start)
            {
                *slot = ch;
            }
        } else {
            let map = |idx: usize| idx * slot_count / before_count;
            let s = map(start);
            let e = map(end);
            for slot in slots
                .iter_mut()
                .take(e.min(slot_count.saturating_sub(1)) + 1)
                .skip(s)
            {
                *slot = ch;
            }
        }
    }

    format!("[{}]", slots.iter().collect::<String>())
}

fn format_action_summary(
    level: u8,
    sorted_actions: &[&crate::types::CompactionAction],
    messages_dropped: usize,
    after_message_count: usize,
) -> String {
    match level {
        1 => {
            let outline_count = sorted_actions
                .iter()
                .filter(|a| a.method == "Outline")
                .count();
            let headtail_count = sorted_actions
                .iter()
                .filter(|a| a.method == "HeadTail")
                .count();
            let mut parts = Vec::new();
            if outline_count > 0 {
                parts.push(format!("outlined {outline_count}"));
            }
            if headtail_count > 0 {
                parts.push(format!("head-tail {headtail_count}"));
            }
            if parts.is_empty() {
                "↓ no changes".to_string()
            } else {
                format!("↓ {}", parts.join(", "))
            }
        }
        2 => {
            let turn_count = sorted_actions.len();
            let total_msgs: usize = sorted_actions
                .iter()
                .map(|a| 1 + a.related_count.unwrap_or(0))
                .sum();
            format!("↓ summarized {turn_count} turns ({total_msgs} msgs → {turn_count} summaries)")
        }
        3 => {
            let kept = after_message_count.saturating_sub(1);
            format!("↓ dropped {messages_dropped} msgs, kept {kept} + 1 marker")
        }
        _ => "↓ no changes".to_string(),
    }
}

impl Default for ReplSink {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(SpinnerState::new())))
    }
}
