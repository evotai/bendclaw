use crate::observability::server_log;
use crate::sessions::Message;

pub(crate) struct ContextPreview {
    pub(crate) previous_user: String,
    pub(crate) previous_assistant: String,
    pub(crate) role_counts: String,
    pub(crate) source_counts: String,
    pub(crate) repeated_input_count: usize,
}

pub(crate) struct HistoryLoadSummary {
    pub(crate) last_user: String,
    pub(crate) last_assistant: String,
    pub(crate) checkpoint_run_id: String,
    pub(crate) replayed_user_messages: usize,
    pub(crate) replayed_assistant_messages: usize,
    pub(crate) replayed_run_ids: String,
    pub(crate) checkpoint_summary_bytes: usize,
}

impl ContextPreview {
    pub(crate) fn from_history(
        prior_history: &[Message],
        history: &[Message],
        current_input: &str,
        current_run_id: &str,
    ) -> Self {
        let repeated_inputs = repeated_prior_input_run_ids(history, current_input, current_run_id);
        Self {
            previous_user: last_role_preview(prior_history, crate::types::Role::User),
            previous_assistant: last_role_preview(prior_history, crate::types::Role::Assistant),
            role_counts: role_count_summary(history),
            source_counts: source_count_summary(history, current_run_id),
            repeated_input_count: repeated_inputs.len(),
        }
    }
}

pub(crate) fn summarize_loaded_history(
    seeded: &[Message],
    checkpoint: Option<&crate::storage::dal::run::record::RunRecord>,
) -> HistoryLoadSummary {
    let replayed_run_ids = seeded
        .iter()
        .filter_map(Message::origin_run_id)
        .fold(Vec::<String>::new(), |mut acc, run_id| {
            if !acc.iter().any(|existing| existing == run_id) {
                acc.push(run_id.to_string());
            }
            acc
        })
        .join(",");

    HistoryLoadSummary {
        last_user: last_role_preview(seeded, crate::types::Role::User),
        last_assistant: last_role_preview(seeded, crate::types::Role::Assistant),
        checkpoint_run_id: checkpoint
            .map(|run| run.checkpoint_through_run_id.clone())
            .unwrap_or_default(),
        replayed_user_messages: seeded
            .iter()
            .filter(|msg| msg.role() == Some(crate::types::Role::User))
            .count(),
        replayed_assistant_messages: seeded
            .iter()
            .filter(|msg| msg.role() == Some(crate::types::Role::Assistant))
            .count(),
        replayed_run_ids,
        checkpoint_summary_bytes: checkpoint.map(|run| run.output.len()).unwrap_or(0),
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
        replayed_user_messages = summary.replayed_user_messages,
        replayed_assistant_messages = summary.replayed_assistant_messages,
        replayed_run_ids = %summary.replayed_run_ids,
        checkpoint_summary_bytes = summary.checkpoint_summary_bytes as u64,
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
        role_counts = %preview.role_counts,
        source_counts = %preview.source_counts,
        repeated_input_count = preview.repeated_input_count as u64,
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

pub(crate) fn log_session_resolved(
    session_id: &str,
    base_key: &str,
    source: &str,
    mode: Option<&str>,
) {
    crate::observability::log::slog!(info, "session", "resolved",
        session_id = %session_id,
        base_key,
        source,
        mode = %mode.unwrap_or(""),
    );
}

pub(crate) fn log_session_replaced(
    base_key: &str,
    previous_session_id: &str,
    session_id: &str,
    reset_reason: &str,
) {
    crate::observability::log::slog!(info, "session", "replaced",
        base_key,
        previous_session_id = %previous_session_id,
        session_id = %session_id,
        reset_reason,
    );
}

pub(crate) fn log_session_started(base_key: &str, session_id: &str, reset_reason: &str) {
    crate::observability::log::slog!(info, "session", "started",
        base_key,
        session_id = %session_id,
        reset_reason,
    );
}

pub(crate) fn log_session_created(session_id: &str, user_id: &str) {
    crate::observability::log::slog!(info, "session", "created",
        session_id = %session_id,
        user_id,
        base_key = "",
    );
}

fn last_role_preview(history: &[Message], role: crate::types::Role) -> String {
    history
        .iter()
        .rev()
        .find(|msg| msg.role() == Some(role.clone()))
        .map(|msg| server_log::preview_text(&msg.text()))
        .unwrap_or_default()
}

fn role_count_summary(history: &[Message]) -> String {
    let system = history
        .iter()
        .filter(|msg| msg.role() == Some(crate::types::Role::System))
        .count();
    let user = history
        .iter()
        .filter(|msg| msg.role() == Some(crate::types::Role::User))
        .count();
    let assistant = history
        .iter()
        .filter(|msg| msg.role() == Some(crate::types::Role::Assistant))
        .count();
    let tool = history
        .iter()
        .filter(|msg| msg.role() == Some(crate::types::Role::Tool))
        .count();
    format!("system:{system},user:{user},assistant:{assistant},tool:{tool}")
}

fn source_count_summary(history: &[Message], current_run_id: &str) -> String {
    let mut checkpoint = 0usize;
    let mut history_replay = 0usize;
    let mut current_run = 0usize;
    let mut tool_result = 0usize;
    let mut runtime = 0usize;

    for msg in history {
        match message_source(msg, current_run_id) {
            "checkpoint" => checkpoint += 1,
            "history_replay" => history_replay += 1,
            "current_run" => current_run += 1,
            "tool_result" => tool_result += 1,
            _ => runtime += 1,
        }
    }

    format!(
        "checkpoint:{checkpoint},history_replay:{history_replay},current_run:{current_run},tool_result:{tool_result},runtime:{runtime}"
    )
}

fn repeated_prior_input_run_ids(
    history: &[Message],
    current_input: &str,
    current_run_id: &str,
) -> Vec<String> {
    history
        .iter()
        .filter_map(|msg| match msg {
            Message::User { .. } if msg.text() == current_input => msg.origin_run_id(),
            _ => None,
        })
        .filter(|run_id| *run_id != current_run_id)
        .fold(Vec::<String>::new(), |mut acc, run_id| {
            if !acc.iter().any(|existing| existing == run_id) {
                acc.push(run_id.to_string());
            }
            acc
        })
}

fn message_source(msg: &Message, current_run_id: &str) -> &'static str {
    match msg {
        Message::CompactionSummary { .. } => "checkpoint",
        Message::ToolResult { .. } if msg.origin_run_id() == Some(current_run_id) => "tool_result",
        Message::User { .. } | Message::Assistant { .. }
            if msg.origin_run_id() == Some(current_run_id) =>
        {
            "current_run"
        }
        Message::User { .. } | Message::Assistant { .. } | Message::ToolResult { .. } => {
            "history_replay"
        }
        _ => "runtime",
    }
}
