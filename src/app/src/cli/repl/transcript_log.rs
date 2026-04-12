use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;

use super::render::build_run_summary;
use super::render::count_messages_by_role;
use super::render::human_tokens;
use crate::agent::AssistantBlock;
use crate::agent::RunEvent;
use crate::agent::RunEventPayload;
use crate::cli::format::format_tool_input_lines;
use crate::cli::format::summarize_inline;

// ---------------------------------------------------------------------------
// TranscriptLog
// ---------------------------------------------------------------------------

pub struct TranscriptLog {
    path: PathBuf,
}

impl TranscriptLog {
    pub fn open(session_id: &str) -> Option<Self> {
        let dir = crate::conf::paths::state_root_dir().ok()?.join("logs");
        if fs::create_dir_all(&dir).is_err() {
            return None;
        }
        let path = dir.join(format!("{session_id}.log"));
        Some(Self { path })
    }

    /// Create a log writer at an explicit path (for testing).
    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write_event(&self, event: &RunEvent) {
        let lines = format_event(&event.payload);
        if lines.is_empty() {
            return;
        }
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let ts = format_timestamp(&event.created_at);
        for (i, line) in lines.iter().enumerate() {
            if i == 0 && !line.is_empty() {
                let _ = writeln!(file, "[{ts}] {line}");
            } else {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    pub fn write_user_prompt(&self, prompt: &str) {
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let ts = format_timestamp(&Utc::now().to_rfc3339());
        let _ = writeln!(file, "[{ts}] > {prompt}");
        let _ = writeln!(file);
    }
}

// ---------------------------------------------------------------------------
// Event formatting (pure plaintext, no ANSI)
// ---------------------------------------------------------------------------

/// Format an RFC3339 timestamp to a compact local-friendly form: `HH:MM:SS`.
/// Falls back to the raw string if parsing fails.
pub fn format_timestamp(rfc3339: &str) -> String {
    match DateTime::parse_from_rfc3339(rfc3339) {
        Ok(dt) => {
            let local = dt.with_timezone(&chrono::Local);
            local.format("%H:%M:%S").to_string()
        }
        Err(_) => rfc3339.to_string(),
    }
}

pub fn format_event(payload: &RunEventPayload) -> Vec<String> {
    match payload {
        RunEventPayload::RunStarted {} => vec![],

        RunEventPayload::TurnStarted {} => vec![],

        RunEventPayload::AssistantDelta { .. } => vec![],

        RunEventPayload::AssistantCompleted {
            content,
            stop_reason,
            error_message,
            ..
        } => {
            let mut lines = Vec::new();
            for block in content {
                match block {
                    AssistantBlock::Text { text } => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            lines.push(trimmed.to_string());
                            lines.push(String::new());
                        }
                    }
                    AssistantBlock::ToolCall { name, input, .. } => {
                        lines.push(format!("[{name} call]"));
                        for param_line in format_tool_input_lines(input) {
                            lines.push(format!("  {param_line}"));
                        }
                        lines.push(String::new());
                    }
                    AssistantBlock::Thinking { .. } => {}
                }
            }
            if stop_reason == "error" {
                let msg = error_message.as_deref().unwrap_or("Unknown error");
                lines.push(format!("[error] {msg}"));
                lines.push(String::new());
            }
            lines
        }

        RunEventPayload::ToolStarted {
            tool_name, args, ..
        } => {
            let mut lines = vec![format!("[{tool_name} call]")];
            for param_line in format_tool_input_lines(args) {
                lines.push(format!("  {param_line}"));
            }
            lines.push(String::new());
            lines
        }

        RunEventPayload::ToolProgress { .. } => vec![],

        RunEventPayload::ToolFinished {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let status = if *is_error { "failed" } else { "completed" };
            let mut lines = vec![format!("[{tool_name} {status}]")];
            let trimmed = content.trim();
            if trimmed.is_empty() {
                if *is_error {
                    lines.push("  tool returned an error".into());
                }
            } else {
                lines.push(format!("  {}", summarize_inline(trimmed, 200)));
            }
            lines.push(String::new());
            lines
        }

        RunEventPayload::LlmCallStarted {
            turn,
            attempt,
            model,
            messages,
            tools,
            system_prompt_tokens,
            ..
        } => {
            let attempt_str = if *attempt > 0 {
                format!(" retry {attempt}")
            } else {
                String::new()
            };
            let stats = count_messages_by_role(messages);
            let mut lines = vec![format!(
                "[llm call] {model} · turn {turn}{attempt_str} · {} messages · {} tools · ~{} est tokens",
                stats.total_count(),
                tools.len(),
                stats.total_tokens(*system_prompt_tokens),
            )];
            lines.push(String::new());
            lines
        }

        RunEventPayload::LlmCallCompleted {
            usage,
            cache_read,
            cache_write,
            error,
            metrics,
            ..
        } => {
            if let Some(err) = error {
                vec![format!("[llm error] {err}"), String::new()]
            } else {
                let mut line = format!(
                    "[llm completed] {} input · {} output tokens",
                    usage.input, usage.output,
                );
                if let Some(m) = metrics {
                    if m.duration_ms > 0 {
                        line.push_str(&format!(" · {}ms", m.duration_ms));
                    }
                    if m.ttft_ms > 0 {
                        line.push_str(&format!(" · ttft {}ms", m.ttft_ms));
                    }
                    if m.streaming_ms > 0 && usage.output > 0 {
                        let tok_per_sec = usage.output as f64 / (m.streaming_ms as f64 / 1000.0);
                        line.push_str(&format!(" · {:.0} tok/s", tok_per_sec));
                    }
                }
                if *cache_read > 0 || *cache_write > 0 {
                    line.push_str(&format!(" · cache r:{cache_read} w:{cache_write}"));
                }
                vec![line, String::new()]
            }
        }

        RunEventPayload::ContextCompactionStarted {
            message_count,
            estimated_tokens,
            budget_tokens,
            ..
        } => {
            let usage_pct = if *budget_tokens > 0 {
                *estimated_tokens as f64 / *budget_tokens as f64 * 100.0
            } else {
                0.0
            };
            let h_est = human_tokens(*estimated_tokens);
            vec![
                format!(
                    "[compact] {message_count} messages · ~{h_est} tokens · {usage_pct:.0}% of budget"
                ),
                String::new(),
            ]
        }

        RunEventPayload::ContextCompactionCompleted { result } => match result {
            crate::types::CompactionResult::LevelCompacted {
                level,
                after_message_count,
                after_estimated_tokens,
                before_estimated_tokens,
                ..
            } => {
                let saved = before_estimated_tokens.saturating_sub(*after_estimated_tokens);
                let h_after = human_tokens(*after_estimated_tokens);
                let h_saved = human_tokens(saved);
                vec![
                    format!(
                        "[compact completed] level {level} · {after_message_count} messages · ~{h_after} tokens · saved ~{h_saved}"
                    ),
                    String::new(),
                ]
            }
            crate::types::CompactionResult::RunOnceCleared {
                cleared_count,
                saved_tokens,
                ..
            } => vec![
                format!(
                    "[compact completed] run-once cleared {cleared_count} tool result(s) · saved ~{}",
                    human_tokens(*saved_tokens)
                ),
                String::new(),
            ],
            crate::types::CompactionResult::NoOp => vec![
                "[compact completed] no compaction needed".into(),
                String::new(),
            ],
        },

        RunEventPayload::RunFinished {
            usage,
            turn_count,
            duration_ms,
            ..
        } => {
            let summary = build_run_summary(usage, *turn_count, *duration_ms, 0, 0);
            vec![format!("---"), summary, String::new()]
        }

        RunEventPayload::Error { message } => {
            vec![format!("[error] {message}"), String::new()]
        }
    }
}
