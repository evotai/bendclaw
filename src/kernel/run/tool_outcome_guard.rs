//! Transient grounding guard for post-tool LLM turns.
//!
//! After a batch of tool calls executes, the next LLM turn receives a
//! short system message summarizing what actually happened. This helps keep
//! user-visible claims aligned with verified tool outcomes without
//! hard-coding task-specific logic.

#[derive(Debug, Clone, Default)]
pub struct ToolOutcomeGuard {
    pending_grounding: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolDispatchReport {
    pub requested: Vec<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    pub blocked: Vec<String>,
    pub skipped: Vec<String>,
}

impl ToolOutcomeGuard {
    pub fn record(&mut self, report: ToolDispatchReport) {
        self.pending_grounding = build_grounding_message(&report);
    }

    pub fn take_grounding(&mut self) -> Option<String> {
        self.pending_grounding.take()
    }
}

fn build_grounding_message(report: &ToolDispatchReport) -> Option<String> {
    if report.requested.is_empty() {
        return None;
    }

    let mut lines = vec![
        "[Execution grounding] Base any completion/status claims only on verified tool outcomes from the immediately preceding turn.".to_string(),
        "Do not claim an action was completed, triggered, sent, updated, deleted, or saved unless a successful tool execution below confirms it.".to_string(),
        format!("Requested tools: {}.", join_names(&report.requested)),
    ];

    if !report.succeeded.is_empty() {
        lines.push(format!(
            "Successful tools: {}.",
            join_names(&report.succeeded)
        ));
    }
    if !report.failed.is_empty() {
        lines.push(format!("Failed tools: {}.", join_names(&report.failed)));
    }
    if !report.blocked.is_empty() {
        lines.push(format!(
            "Blocked tools (not executed): {}.",
            join_names(&report.blocked)
        ));
    }
    if !report.skipped.is_empty() {
        lines.push(format!(
            "Skipped tools (not executed): {}.",
            join_names(&report.skipped)
        ));
    }

    Some(lines.join("\n"))
}

fn join_names(names: &[String]) -> String {
    names.join(", ")
}
