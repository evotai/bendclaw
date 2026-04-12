use std::io::Stdout;
use std::io::Write;
use std::sync::OnceLock;

use parking_lot::Mutex;

use crate::agent::TranscriptItem;
use crate::agent::UsageSummary;
pub use crate::cli::format::format_tool_input;
pub use crate::cli::format::format_tool_input_lines;
pub use crate::cli::format::summarize_inline;
pub use crate::cli::format::truncate;
pub use crate::cli::format::truncate_head_tail;
pub use crate::types::count_messages_by_role;
pub use crate::types::CompactRecord;
pub use crate::types::MessageStats;
pub use crate::types::RunSummaryData;
pub use crate::types::ToolAggStats;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const BLACK: &str = "\x1b[30m";
pub const WHITE: &str = "\x1b[37m";
pub const GRAY: &str = "\x1b[90m";
pub const BG_TOOL: &str = "\x1b[48;2;245;197;66m";
pub const BG_OK: &str = "\x1b[48;2;133;220;140m";
pub const BG_ERR: &str = "\x1b[48;2;157;57;57m";

static TERMINAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn with_terminal<T>(render: impl FnOnce(&mut Stdout) -> T) -> T {
    let _guard = TERMINAL_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut stdout = std::io::stdout();
    let result = render(&mut stdout);
    let _ = stdout.flush();
    result
}

pub fn terminal_write(text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
    with_terminal(|stdout| {
        let _ = write!(stdout, "{normalized}");
    });
}

pub fn terminal_writeln(text: &str) {
    terminal_write(text);
    terminal_write("\r\n");
}

pub fn terminal_prefixed_writeln(text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let output = format!("{DIM}•{RESET} {normalized}\r\n");
    with_terminal(|stdout| {
        let _ = write!(stdout, "{output}");
    });
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

pub fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

pub fn build_run_summary(
    usage: &UsageSummary,
    turn_count: u32,
    duration_ms: u64,
    llm_calls: u32,
    tool_calls: u32,
) -> String {
    let total_tokens = usage.input + usage.output;

    let mut parts = vec![
        format!("run {}", human_duration(duration_ms)),
        format!("turns {}", turn_count),
    ];
    if llm_calls > 0 {
        parts.push(format!("llm {}", llm_calls));
    }
    if tool_calls > 0 {
        parts.push(format!("tools {}", tool_calls));
    }
    parts.push(format!(
        "tokens {} (in {} · out {})",
        total_tokens, usage.input, usage.output
    ));
    let hit_rate = usage.cache_hit_rate();
    if hit_rate > 0.0 {
        parts.push(format!("cache {:.0}%", hit_rate * 100.0));
    }

    parts.join("  ·  ")
}

/// Format detail lines for the LLM completed badge.
///
/// Returns lines like:
///   tokens   61001 in · 248 out · 108 tok/s
///   timing   3.2s · ttfb 245ms (8%) · ttft 892ms (28%) · stream 2.3s (72%)
pub fn format_llm_completed_lines(
    usage: &UsageSummary,
    metrics: Option<&crate::agent::LlmCallMetrics>,
) -> Vec<String> {
    let mut lines = Vec::new();

    // Line 1: tokens
    let mut token_line = format!("tokens   {} in · {} out", usage.input, usage.output,);
    if let Some(m) = metrics {
        if m.streaming_ms > 0 && usage.output > 0 {
            let tok_per_sec = usage.output as f64 / (m.streaming_ms as f64 / 1000.0);
            token_line.push_str(&format!(" · {:.0} tok/s", tok_per_sec));
        }
    }
    lines.push(token_line);

    // Line 2: timing (only if metrics available and duration > 0)
    if let Some(m) = metrics {
        if m.duration_ms > 0 {
            let dur = m.duration_ms as f64;
            let mut parts = vec![human_duration(m.duration_ms)];

            if m.ttfb_ms > 0 {
                let pct = m.ttfb_ms as f64 / dur * 100.0;
                parts.push(format!("ttfb {} ({:.0}%)", human_duration(m.ttfb_ms), pct));
            }
            if m.ttft_ms > 0 {
                let pct = m.ttft_ms as f64 / dur * 100.0;
                parts.push(format!("ttft {} ({:.0}%)", human_duration(m.ttft_ms), pct));
            }
            if m.streaming_ms > 0 {
                let pct = m.streaming_ms as f64 / dur * 100.0;
                parts.push(format!(
                    "stream {} ({:.0}%)",
                    human_duration(m.streaming_ms),
                    pct
                ));
            }

            lines.push(format!("timing   {}", parts.join(" · ")));
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// Run summary rendering
// ---------------------------------------------------------------------------

/// Human-friendly token count: "312k", "1.2m", "800".
pub fn human_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}

/// Render a simple bar: `████░░░░` of given width based on ratio (0.0–1.0).
pub fn render_bar(ratio: f64, width: usize) -> String {
    let capped = ratio.clamp(0.0, 1.0);
    let filled = (capped * width as f64).round() as usize;
    (0..width)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect()
}

/// Format the full run summary block.
pub fn format_run_summary(data: &RunSummaryData) -> Vec<String> {
    let mut lines = Vec::new();
    let total_input = data.usage.input as usize;
    let bar_width = 20;

    // Header
    lines.push("─── This Run Summary ──────────────────────────────────".into());
    let header = format!(
        "{} · {} turns · {} llm calls · {} tool calls · {} tokens",
        human_duration(data.duration_ms),
        data.turn_count,
        data.llm_call_count,
        data.tool_call_count,
        total_input + data.usage.output as usize,
    );
    lines.push(header);
    if let Some((estimated, budget)) = data.last_context_budget {
        if budget > 0 {
            let bar = format_budget_bar(estimated, budget, bar_width);
            lines.push(format!("  context   {bar}"));
        }
    }
    lines.push(String::new());

    // --- tokens block ---
    let total_output = data.usage.output as usize;
    let tok_per_sec = if !data.llm_metrics.is_empty() {
        let total_stream: u64 = data.llm_metrics.iter().map(|m| m.streaming_ms).sum();
        if total_stream > 0 {
            Some(total_output as f64 / (total_stream as f64 / 1000.0))
        } else {
            None
        }
    } else {
        None
    };

    let mut tok_line = format!(
        "  tokens    {} total input · {} output",
        total_input, total_output,
    );
    if let Some(tps) = tok_per_sec {
        tok_line.push_str(&format!(" · {:.1} tok/s", tps));
    }
    lines.push(tok_line);

    // Token breakdown from last LLM call's message stats
    if let Some(ref stats) = data.last_message_stats {
        let sys = data.system_prompt_tokens;
        let user = stats.user_tokens;
        let asst = stats.assistant_tokens;
        let tool = stats.tool_result_tokens;

        let max_label_width = 12;
        let max_val_width = 8;

        for (label, tokens) in [
            ("system", sys),
            ("user", user),
            ("assistant", asst),
            ("tool_result", tool),
        ] {
            if tokens == 0 {
                continue;
            }
            let pct = if total_input > 0 {
                tokens as f64 / total_input as f64 * 100.0
            } else {
                0.0
            };
            let bar = render_bar(pct / 100.0, bar_width);
            lines.push(format!(
                "            {:<width_l$} {:>width_v$}  {bar} {pct:>5.1}%",
                label,
                human_tokens(tokens),
                width_l = max_label_width,
                width_v = max_val_width,
            ));
        }

        // Per-tool breakdown under tool_result.
        // Align bars with parent rows whose bar starts at column
        // 12 indent + 12 label + 1 space + 8 tokens + 2 gap = 35.
        // Sub-tool indent is 14, so minimum left content = 35 - 14 = 21.
        // If actual content is wider, all sub-tool bars shift right together.
        if !data.tool_stats.is_empty() {
            let sub_indent: usize = 14;
            let min_left = 12 + max_label_width + 1 + max_val_width + 2 - sub_indent;
            let max_content: usize = data
                .tool_stats
                .iter()
                .map(|(n, agg)| {
                    let cw = if agg.calls == 1 { "call" } else { "calls" };
                    // "name  N call(s)  Tk"
                    n.len()
                        + 2
                        + format!("{}", agg.calls).len()
                        + 1
                        + cw.len()
                        + 2
                        + human_tokens(agg.result_tokens).len()
                })
                .max()
                .unwrap_or(0);
            let left_width = min_left.max(max_content);
            for (name, agg) in &data.tool_stats {
                let pct = if total_input > 0 {
                    agg.result_tokens as f64 / total_input as f64 * 100.0
                } else {
                    0.0
                };
                let bar = render_bar(pct / 100.0, bar_width);
                let call_word = if agg.calls == 1 { "call" } else { "calls" };
                let left = format!(
                    "{}  {} {}  {}",
                    name,
                    agg.calls,
                    call_word,
                    human_tokens(agg.result_tokens),
                );
                lines.push(format!(
                    "{:indent$}{:<width$}{bar} {pct:>5.1}%",
                    "",
                    left,
                    indent = sub_indent,
                    width = left_width,
                ));
            }
        }
    }
    lines.push(String::new());

    // --- compact block ---
    if !data.compact_history.is_empty() {
        let real_compacts: Vec<&CompactRecord> = data
            .compact_history
            .iter()
            .filter(|c| c.level > 0)
            .collect();
        let run_once_compacts: Vec<&CompactRecord> = data
            .compact_history
            .iter()
            .filter(|c| c.level == 0)
            .collect();

        let total_saved: usize = data
            .compact_history
            .iter()
            .map(|c| c.before_tokens.saturating_sub(c.after_tokens))
            .sum();

        lines.push(format!(
            "  compact   {} compactions · saved {} tokens",
            data.compact_history.len(),
            human_tokens(total_saved),
        ));
        for c in &run_once_compacts {
            let saved = c.before_tokens.saturating_sub(c.after_tokens);
            lines.push(format!(
                "            run-once  {}→{}  saved {}",
                human_tokens(c.before_tokens),
                human_tokens(c.after_tokens),
                human_tokens(saved),
            ));
        }
        for (i, c) in real_compacts.iter().enumerate() {
            let saved = c.before_tokens.saturating_sub(c.after_tokens);
            let pct = if c.before_tokens > 0 {
                saved as f64 / c.before_tokens as f64 * 100.0
            } else {
                0.0
            };
            let bar = render_bar(pct / 100.0, 12);
            lines.push(format!(
                "            #{}  lv{}  {}→{}  saved {}  {bar} {pct:.0}%",
                i + 1,
                c.level,
                human_tokens(c.before_tokens),
                human_tokens(c.after_tokens),
                human_tokens(saved),
            ));
        }
        lines.push(String::new());
    }

    // --- llm block ---
    if !data.llm_metrics.is_empty() {
        let total_llm_ms: u64 = data.llm_metrics.iter().map(|m| m.duration_ms).sum();
        let llm_pct = if data.duration_ms > 0 {
            total_llm_ms as f64 / data.duration_ms as f64 * 100.0
        } else {
            0.0
        };
        let total_output_tokens: u64 = data.llm_output_tokens.iter().sum();
        let total_stream_ms: u64 = data.llm_metrics.iter().map(|m| m.streaming_ms).sum();
        let avg_tps = if total_stream_ms > 0 {
            total_output_tokens as f64 / (total_stream_ms as f64 / 1000.0)
        } else {
            0.0
        };

        lines.push(format!(
            "  llm       {} calls · {} ({:.0}% of run) · {:.1} tok/s avg",
            data.llm_metrics.len(),
            human_duration(total_llm_ms),
            llm_pct,
            avg_tps,
        ));

        let count = data.llm_metrics.len() as u64;
        let total_ttft: u64 = data.llm_metrics.iter().map(|m| m.ttft_ms).sum();
        let total_stream: u64 = data.llm_metrics.iter().map(|m| m.streaming_ms).sum();
        let avg_ttft = if count > 0 { total_ttft / count } else { 0 };
        let avg_stream = if count > 0 { total_stream / count } else { 0 };
        lines.push(format!(
            "            ttft avg {} · stream avg {}",
            human_duration(avg_ttft),
            human_duration(avg_stream),
        ));

        // Top 3 LLM calls by duration
        let mut indexed: Vec<(usize, u64)> = data
            .llm_metrics
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.duration_ms))
            .collect();
        indexed.sort_by(|a, b| b.1.cmp(&a.1));

        let max_dur = indexed.first().map(|(_, d)| *d).unwrap_or(1);
        let show = indexed.len().min(3);

        // Pre-compute max widths for alignment
        let max_idx_width = indexed[..show]
            .iter()
            .map(|(i, _)| format!("#{}", i + 1).len())
            .max()
            .unwrap_or(2);
        let max_dur_width = indexed[..show]
            .iter()
            .map(|(_, d)| human_duration(*d).len())
            .max()
            .unwrap_or(4);

        for &(idx, dur) in &indexed[..show] {
            let bar = render_bar(dur as f64 / max_dur as f64, bar_width);
            let pct = if total_llm_ms > 0 {
                dur as f64 / total_llm_ms as f64 * 100.0
            } else {
                0.0
            };
            let idx_str = format!("#{}", idx + 1);
            lines.push(format!(
                "            {:<idx_w$}  {:>dur_w$} {bar} {pct:>3.0}%",
                idx_str,
                human_duration(dur),
                idx_w = max_idx_width,
                dur_w = max_dur_width,
            ));
        }
        if indexed.len() > 3 {
            let rest_count = indexed.len() - 3;
            let rest_ms: u64 = indexed[3..].iter().map(|(_, d)| *d).sum();
            lines.push(format!(
                "            ... {} more calls · {} total",
                rest_count,
                human_duration(rest_ms),
            ));
        }
    }

    // Footer
    lines.push("────────────────────────────────────────────────────────".into());

    lines
}

// ---------------------------------------------------------------------------
// Transcript rendering
// ---------------------------------------------------------------------------

pub fn print_transcript_messages(items: &[TranscriptItem]) {
    for item in items {
        match item {
            TranscriptItem::User { text } => {
                if !text.trim().is_empty() {
                    println!("{YELLOW}> {RESET}{}", text.trim());
                    println!();
                }
            }
            TranscriptItem::Assistant {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    let _ = super::markdown::render_markdown(text.trim());
                    terminal_writeln("");
                }
                for tc in tool_calls {
                    print_tool_call(&tc.name, &tc.input, None);
                }
            }
            TranscriptItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                print_tool_result(tool_name, content, *is_error, None);
            }
            _ => {}
        }
    }
}

pub fn print_tool_call(name: &str, input: &serde_json::Value, preview_command: Option<&str>) {
    let title = format!("{name} call");
    print_badge_line(&title, false, false);
    if let Some(cmd) = preview_command {
        // preview_command already contains the full operation info; skip redundant param lines
        terminal_writeln(&format!("{GRAY}  ❯ {cmd}{RESET}"));
    } else {
        // No preview_command (e.g. web_fetch): fall back to parameter lines
        let lines = format_tool_input_lines(input);
        for line in lines {
            terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
        }
    }
    terminal_writeln("");
}

pub fn print_tool_result(
    tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) {
    let title = if is_error {
        format!("{tool_name} failed")
    } else {
        format!("{tool_name} completed")
    };
    let lines = tool_result_lines(content, is_error, tool_call);
    print_badge_line(&title, true, !is_error);
    let color = if is_error { RED } else { GREEN };
    for line in lines {
        terminal_writeln(&format!("{color}  {line}{RESET}"));
    }
    terminal_writeln("");
}

pub fn print_badge_line(title: &str, is_result: bool, ok: bool) {
    let (badge, rest) = split_tool_title(title);
    let (fg, bg) = if is_result {
        if ok {
            (BLACK, BG_OK)
        } else {
            (WHITE, BG_ERR)
        }
    } else {
        (BLACK, BG_TOOL)
    };

    if rest.is_empty() {
        terminal_writeln(&format!("{bg}{fg}{BOLD}[{badge}]{RESET}"));
    } else {
        terminal_writeln(&format!(
            "{bg}{fg}{BOLD}[{badge}]{RESET} {GRAY}{rest}{RESET}"
        ));
    }
}

pub fn tool_result_lines(
    content: &str,
    is_error: bool,
    _tool_call: Option<&ToolCallSummary>,
) -> Vec<String> {
    let summarize = || -> String {
        if content.trim().is_empty() {
            if is_error {
                "Result: tool returned an error".into()
            } else {
                "Result: completed".into()
            }
        } else {
            format!("Result: {}", summarize_inline(content, 160))
        }
    };

    const HEAD_LINES: usize = 5;
    const TAIL_LINES: usize = 3;
    const COMPACT_THRESHOLD: usize = HEAD_LINES + TAIL_LINES + 2;
    const MAX_LINE_WIDTH: usize = 256;

    let cap_line = |l: &str| -> String { truncate_head_tail(l, MAX_LINE_WIDTH) };

    let normalized = content.replace("\r\n", "\n");
    if normalized.contains('\n') {
        let trimmed = normalized.trim_end_matches('\n');
        if trimmed.is_empty() {
            return vec![summarize()];
        }
        let all_lines: Vec<&str> = trimmed.split('\n').collect();
        if all_lines.len() > COMPACT_THRESHOLD {
            let mut result: Vec<String> = Vec::new();
            result.extend(all_lines[..HEAD_LINES].iter().map(|l| cap_line(l)));
            let omitted = all_lines.len() - HEAD_LINES - TAIL_LINES;
            result.push(format!("... ({omitted} more lines)"));
            result.extend(
                all_lines[all_lines.len() - TAIL_LINES..]
                    .iter()
                    .map(|l| cap_line(l)),
            );
            return result;
        }
        return all_lines.into_iter().map(cap_line).collect();
    }
    vec![summarize()]
}

pub fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
}

/// Minimal summary of a tool call used for result display.
pub struct ToolCallSummary {
    pub name: String,
    pub summary: String,
}

/// Format detail lines for an LLM call badge.
pub fn format_llm_call_lines(
    stats: &MessageStats,
    tool_count: usize,
    system_prompt_tokens: usize,
) -> Vec<String> {
    let mut lines = Vec::new();

    // Line 1: message counts
    let total = stats.total_count();
    let mut role_parts = Vec::new();
    if stats.user_count > 0 {
        role_parts.push(format!("user {}", stats.user_count));
    }
    if stats.assistant_count > 0 {
        role_parts.push(format!("assistant {}", stats.assistant_count));
    }
    if stats.tool_result_count > 0 {
        role_parts.push(format!("tool_result {}", stats.tool_result_count));
    }
    let msg_line = if role_parts.is_empty() {
        format!("{total} messages · {tool_count} tools")
    } else {
        format!(
            "{total} messages ({}) · {tool_count} tools",
            role_parts.join(" · ")
        )
    };
    lines.push(msg_line);

    // Line 2: token estimates by role
    let est_total = stats.total_tokens(system_prompt_tokens);
    let mut token_parts = vec![format!("sys ~{}", human_tokens(system_prompt_tokens))];
    if stats.user_tokens > 0 {
        token_parts.push(format!("user ~{}", human_tokens(stats.user_tokens)));
    }
    if stats.assistant_tokens > 0 {
        token_parts.push(format!(
            "assistant ~{}",
            human_tokens(stats.assistant_tokens)
        ));
    }
    if stats.tool_result_tokens > 0 {
        token_parts.push(format!(
            "tool_result ~{}",
            human_tokens(stats.tool_result_tokens)
        ));
    }
    lines.push(format!(
        "~{} est tokens ({})",
        human_tokens(est_total),
        token_parts.join(" · ")
    ));

    // Line 3+: per-tool breakdown (only if >= 2 tool results)
    if stats.tool_details.len() >= 2 {
        lines.push(String::new());
        lines.push("tool results:".to_string());
        let breakdown = format_tool_breakdown(&stats.tool_details, stats.tool_result_tokens);
        lines.extend(breakdown);
    }

    lines
}

/// Aggregate same-name tools and sort by tokens descending.
fn aggregate_tool_details(details: &[(String, usize)]) -> Vec<(String, usize)> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for (name, tokens) in details {
        *map.entry(name.clone()).or_default() += tokens;
    }
    let mut agg: Vec<(String, usize)> = map.into_iter().collect();
    agg.sort_by(|a, b| b.1.cmp(&a.1));
    agg
}

/// Format per-tool token breakdown lines (aggregated by tool name).
pub fn format_tool_breakdown(details: &[(String, usize)], total: usize) -> Vec<String> {
    let agg = aggregate_tool_details(details);
    let max_name_len = agg.iter().map(|(n, _)| n.len()).max().unwrap_or(4);
    let bar_width = 20;

    agg.iter()
        .map(|(name, tokens)| {
            let pct = if total > 0 {
                *tokens as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let bar = render_bar(pct / 100.0, bar_width);
            let h_tok = human_tokens(*tokens);
            format!(
                "  {:<width$}  ~{:<8} ({:>5.1}%)  {bar}",
                name,
                h_tok,
                pct,
                width = max_name_len,
            )
        })
        .collect()
}

/// Render a budget usage bar with percentage.
///
/// Output: `███░░░  38%(62k) of budget(156k)`
pub fn format_budget_bar(used: usize, budget: usize, width: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    let ratio = used as f64 / budget as f64;
    let bar = render_bar(ratio, width);
    let h_used = human_tokens(used);
    let h_budget = human_tokens(budget);
    format!(
        "{bar}  {:.0}%({h_used}) of budget({h_budget})",
        ratio * 100.0
    )
}
