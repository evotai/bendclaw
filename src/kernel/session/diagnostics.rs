use crate::kernel::Message;
use crate::observability::server_log;

pub(crate) struct ContextPreview {
    pub(crate) previous_user: String,
    pub(crate) previous_assistant: String,
    pub(crate) history_tail: String,
}

pub(crate) struct HistoryLoadSummary {
    pub(crate) last_user: String,
    pub(crate) last_assistant: String,
    pub(crate) checkpoint_run_id: String,
}

impl ContextPreview {
    pub(crate) fn from_history(history: &[Message]) -> Self {
        Self {
            previous_user: last_role_preview(history, crate::base::Role::User),
            previous_assistant: last_role_preview(history, crate::base::Role::Assistant),
            history_tail: history_tail_summary(history, 6),
        }
    }
}

pub(crate) fn summarize_loaded_history(
    seeded: &[Message],
    checkpoint: Option<&crate::storage::dal::run::record::RunRecord>,
) -> HistoryLoadSummary {
    HistoryLoadSummary {
        last_user: last_role_preview(seeded, crate::base::Role::User),
        last_assistant: last_role_preview(seeded, crate::base::Role::Assistant),
        checkpoint_run_id: checkpoint
            .map(|run| run.checkpoint_through_run_id.clone())
            .unwrap_or_default(),
    }
}

pub(crate) fn log_history_loaded(
    session_id: &str,
    run_count: usize,
    replay_runs: usize,
    seeded_messages: usize,
    summary: &HistoryLoadSummary,
) {
    crate::observability::log::slog!(info, "history", "loaded",
        msg = "session history loaded",
        session_id,
        run_count,
        replay_runs,
        seeded_messages,
        checkpoint_run_id = %summary.checkpoint_run_id,
        last_user = %summary.last_user,
        last_assistant = %summary.last_assistant,
    );
}

pub(crate) fn log_context_prepared(
    run_ctx: server_log::ServerCtx<'_>,
    user_message: &str,
    run_index: u32,
    history: &[Message],
    preview: &ContextPreview,
) {
    crate::observability::log::run_log!(info, run_ctx, "context", "prepared",
        msg = "context prepared",
        history_messages = history.len(),
        prior_turns = run_index.saturating_sub(1),
        current_input = %server_log::preview_text(user_message),
        previous_user = %preview.previous_user,
        previous_assistant = %preview.previous_assistant,
        history_tail = %preview.history_tail,
    );
}

pub(crate) fn log_run_started(
    run_ctx: server_log::ServerCtx<'_>,
    user_id: &str,
    user_message: &str,
    run_index: u32,
    parent_run_id: Option<&str>,
) {
    crate::observability::log::run_log!(info, run_ctx, "run", "started",
        msg = format!("─── RUN {} {} user={} ───",
            server_log::short_run_id(run_ctx.run_id),
            run_ctx.session_id,
            user_id,
        ),
        input_preview = %server_log::preview_text(user_message),
        user_id = %user_id,
        run_index,
        parent_run_id = %parent_run_id.unwrap_or(""),
        bytes = user_message.len() as u64,
    );
}

pub(crate) fn log_prompt_built(
    run_ctx: server_log::ServerCtx<'_>,
    user_id: &str,
    prompt_bytes: usize,
    tool_count: usize,
    history_messages: usize,
) {
    crate::observability::log::run_log!(info, run_ctx, "prompt", "built",
        msg = format!("  prompt: {}B tools={} history={}",
            prompt_bytes,
            tool_count,
            history_messages,
        ),
        bytes = prompt_bytes as u64,
        user_id = %user_id,
        tool_count,
        history_messages,
    );
}

pub(crate) fn log_run_rejected(session_id: &str, agent_id: &str, active_run_id: &str) {
    crate::observability::log::slog!(warn, "session", "run_rejected",
        session_id = %session_id,
        agent_id = %agent_id,
        active_run_id = %active_run_id,
    );
}

fn last_role_preview(history: &[Message], role: crate::base::Role) -> String {
    history
        .iter()
        .rev()
        .find(|msg| msg.role() == Some(role.clone()))
        .map(|msg| server_log::preview_text(&msg.text()))
        .unwrap_or_default()
}

fn history_tail_summary(history: &[Message], limit: usize) -> String {
    let start = history.len().saturating_sub(limit);
    history[start..]
        .iter()
        .filter_map(|msg| {
            let role = msg.role()?;
            let label = match role {
                crate::base::Role::System => "system",
                crate::base::Role::User => "user",
                crate::base::Role::Assistant => "assistant",
                crate::base::Role::Tool => "tool",
            };
            Some(format!(
                "{label}: {}",
                server_log::preview_text(&msg.text())
            ))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
