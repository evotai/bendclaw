use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionOption {
    ContinueCurrent,
    CancelAndSwitch,
    AppendAsFollowup,
}

#[derive(Debug, Clone)]
pub struct PendingDecision {
    pub session_id: String,
    pub active_run_id: String,
    pub question_id: String,
    pub question_text: String,
    pub candidate_input: String,
    pub options: Vec<DecisionOption>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionResolution {
    ContinueCurrent,
    CancelAndSwitch,
    AppendAsFollowup,
}

pub fn resolve_decision(reply: &str) -> DecisionResolution {
    let lower = reply.trim().to_lowercase();
    if lower.contains("switch")
        || lower.contains("cancel")
        || lower.contains("replace")
        || lower.contains("restart")
    {
        DecisionResolution::CancelAndSwitch
    } else if lower.contains("append")
        || lower.contains("after")
        || lower.contains("queue")
        || lower.contains("later")
    {
        DecisionResolution::AppendAsFollowup
    } else {
        // Default: continue current (safe — don't disrupt running work)
        DecisionResolution::ContinueCurrent
    }
}

pub fn clarification_template(active_summary: &str) -> String {
    format!(
        "I am still working on: \"{active_summary}\". What would you like me to do?\n\
        - Reply \"continue\" to keep the current task running\n\
        - Reply \"switch\" to stop it and start your new request\n\
        - Reply \"append\" to queue your message for after the current task finishes"
    )
}
