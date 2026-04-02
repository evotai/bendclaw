#[allow(clippy::too_many_arguments)]
pub(crate) fn log_channel_sent(
    output_bytes: usize,
    elapsed_ms: u64,
    channel_type: &str,
    account_id: &str,
    external_account_id: &str,
    chat_id: &str,
    send_type: &str,
    message_id: &str,
) {
    crate::observability::log::slog!(
        info,
        "channel",
        "sent",
        output_bytes,
        elapsed_ms,
        channel_type = %channel_type,
        account_id = %account_id,
        external_account_id = %external_account_id,
        chat_id,
        send_type,
        message_id,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log_channel_failed(
    output_bytes: usize,
    elapsed_ms: u64,
    error: &impl std::fmt::Display,
    channel_type: &str,
    account_id: &str,
    external_account_id: &str,
    chat_id: &str,
    send_type: &str,
) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "failed",
        output_bytes,
        elapsed_ms,
        error = %error,
        channel_type = %channel_type,
        account_id = %account_id,
        external_account_id = %external_account_id,
        chat_id,
        send_type,
    );
}

pub(crate) fn log_channel_insert_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "channel", "insert_failed", error = %error,);
}

pub(crate) fn log_channel_rejected() {
    crate::observability::log::slog!(warn, "channel", "rejected",);
}

pub(crate) fn log_channel_busy(remaining: usize, threshold: usize) {
    crate::observability::log::slog!(info, "channel", "busy", remaining, threshold,);
}

pub(crate) fn log_channel_queue_full() {
    crate::observability::log::slog!(warn, "channel", "queue_full",);
}

pub(crate) fn log_channel_retry(
    error: &impl std::fmt::Display,
    attempt: usize,
    next_backoff_secs: u64,
) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "retry",
        error = %error,
        attempt,
        next_backoff_secs,
    );
}

pub(crate) fn log_channel_dead_letter() {
    crate::observability::log::slog!(error, "channel", "dead_letter",);
}

pub(crate) fn log_channel_dead_letter_failed(
    error: &impl std::fmt::Display,
    attempt: usize,
    chat_id: &str,
) {
    crate::observability::log::slog!(
        error,
        "channel",
        "dead_letter",
        error = %error,
        attempt,
        chat_id = %chat_id,
    );
}

pub(crate) fn log_channel_send_draft_failed(error: Option<&impl std::fmt::Display>) {
    match error {
        Some(error) => crate::observability::log::slog!(
            warn,
            "channel",
            "send_draft_failed",
            error = %error,
        ),
        None => crate::observability::log::slog!(warn, "channel", "send_draft_failed",),
    }
}

pub(crate) fn log_channel_finalize_draft_failed(error: Option<&impl std::fmt::Display>) {
    match error {
        Some(error) => crate::observability::log::slog!(
            warn,
            "channel",
            "finalize_draft_failed",
            error = %error,
        ),
        None => crate::observability::log::slog!(warn, "channel", "finalize_draft_failed",),
    }
}

pub(crate) fn log_channel_update_draft_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "channel", "update_draft_failed", error = %error,);
}

pub(crate) fn log_channel_stream_timeout(timeout_secs: u64) {
    crate::observability::log::slog!(warn, "channel", "stream_timeout", timeout_secs,);
}

pub(crate) fn log_channel_dispatch_failed(
    agent_id: &str,
    channel_type: &str,
    account_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        error,
        "channel",
        "dispatch_failed",
        agent_id = %agent_id,
        channel_type = %channel_type,
        account_id = %account_id,
        error = %error,
    );
}

pub(crate) fn log_channel_session_reset_failed(
    agent_id: &str,
    channel_type: &str,
    account_id: &str,
    chat_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "session_reset_failed",
        agent_id = %agent_id,
        channel_type = %channel_type,
        account_id = %account_id,
        chat_id,
        error = %error,
    );
}

pub(crate) fn log_channel_dedup_check_failed(
    message_id: &str,
    channel_type: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "dedup_check_failed",
        message_id = %message_id,
        channel_type = %channel_type,
        error = %error,
    );
}

pub(crate) fn log_channel_dedup_skipped(channel_type: &str) {
    crate::observability::log::slog!(info, "channel", "dedup_skipped", channel_type = %channel_type,);
}

pub(crate) fn log_channel_max_restarts_exceeded(channel_account_id: &str, restarts: u32) {
    crate::observability::log::slog!(
        error,
        "channel",
        "max_restarts_exceeded",
        channel_account_id = %channel_account_id,
        restarts,
    );
}

pub(crate) fn log_channel_restarting(channel_account_id: &str, attempt: u32) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "restarting",
        channel_account_id = %channel_account_id,
        attempt,
    );
}

pub(crate) fn log_channel_restart_failed(channel_account_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(
        error,
        "channel",
        "restart_failed",
        channel_account_id = %channel_account_id,
        error = %error,
    );
}

pub(crate) fn log_channel_discover_skipped(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "channel", "discover_skipped", agent_id, error = %error,);
}

pub(crate) fn log_channel_list_failed(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "channel", "list_failed", agent_id, error = %error,);
}

pub(crate) fn log_channel_final_output(
    run_id: &str,
    channel_type: &str,
    account_id: &str,
    chat_id: &str,
    output_preview: &str,
    output_bytes: u64,
) {
    crate::observability::log::slog!(
        info,
        "channel",
        "final_output",
        msg = "channel final output ready",
        run_id = %run_id,
        channel_type,
        account_id,
        chat_id,
        output_preview = %output_preview,
        output_bytes,
    );
}

pub(crate) fn log_channel_retry_after(error: &impl std::fmt::Display, retry_after_ms: u64) {
    crate::observability::log::slog!(
        warn,
        "channel",
        "retry",
        error = %error,
        retry_after_ms,
    );
}

pub(crate) fn log_channel_rate_limited(channel_type: &str, account_id: &str, wait_ms: u64) {
    crate::observability::log::slog!(
        info,
        "channel",
        "rate_limited",
        channel_type,
        account_id,
        wait_ms,
    );
}

pub(crate) fn log_channel_poll_error(account_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(error, "channel", "poll_error", account_id = %account_id, error = %error,);
}

pub(crate) fn log_channel_sent_reaction(
    channel_type: &str,
    chat_id: &str,
    message_id: &str,
    emoji: &str,
) {
    crate::observability::log::slog!(
        info,
        "channel",
        "sent",
        channel_type = channel_type,
        chat_id,
        message_id,
        emoji,
    );
}

pub(crate) fn log_channel_denied(sender_id: i64) {
    crate::observability::log::slog!(warn, "channel", "denied", sender_id,);
}

pub(crate) fn log_channel_dropped() {
    crate::observability::log::slog!(warn, "channel", "dropped",);
}

pub(crate) fn log_channel_reaction_failed(status: reqwest::StatusCode, body: &str) {
    crate::observability::log::slog!(warn, "channel", "reaction_failed", status = %status, body,);
}

pub(crate) fn log_feishu_sender_denied(sender_id: &str) {
    crate::observability::log::slog!(warn, "feishu_ws", "sender_denied", sender_id,);
}

pub(crate) fn log_feishu_content_parse_failed(msg_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "feishu_ws", "content_parse_failed", msg_id, error = %error,);
}

pub(crate) fn log_feishu_unsupported_msg_type(msg_type: &str, msg_id: &str) {
    crate::observability::log::slog!(warn, "feishu_ws", "unsupported_msg_type", msg_type, msg_id,);
}

pub(crate) fn log_feishu_receiver_started(account_id: &str) {
    crate::observability::log::slog!(info, "feishu_ws", "receiver_started", account_id = %account_id,);
}

pub(crate) fn log_feishu_receiver_cancelled(account_id: &str) {
    crate::observability::log::slog!(info, "feishu_ws", "receiver_cancelled", account_id = %account_id,);
}

pub(crate) fn log_feishu_closed_reconnecting(account_id: &str) {
    crate::observability::log::slog!(info, "feishu_ws", "closed_reconnecting", account_id = %account_id,);
}

pub(crate) fn log_feishu_client_error_stopping(account_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(error, "feishu_ws", "client_error_stopping", account_id = %account_id, error = %error,);
}

pub(crate) fn log_feishu_error_reconnecting(
    account_id: &str,
    attempt: u64,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(error, "feishu_ws", "error_reconnecting", account_id = %account_id, attempt, error = %error,);
}

pub(crate) fn log_feishu_reconnect_limit_reached(account_id: &str, limit: u64) {
    crate::observability::log::slog!(error, "feishu_ws", "reconnect_limit_reached", account_id = %account_id, limit,);
}

pub(crate) fn log_feishu_send_failed(code: i64, msg: &str) {
    crate::observability::log::slog!(warn, "feishu_outbound", "send_failed", code, msg,);
}

pub(crate) fn log_feishu_edit_failed(http_status: u16, body: &str) {
    crate::observability::log::slog!(warn, "feishu_outbound", "edit_failed", http_status, body = %body,);
}

pub(crate) fn log_feishu_reaction_token_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "feishu_outbound", "reaction_token_failed", error = %error,);
}

pub(crate) fn log_feishu_reaction_sent(message_id: &str, emoji_type: &str) {
    crate::observability::log::slog!(
        debug,
        "feishu_outbound",
        "reaction_sent",
        message_id,
        emoji_type,
    );
}

pub(crate) fn log_feishu_reaction_failed(
    http_status: reqwest::StatusCode,
    body: &str,
    message_id: &str,
    emoji_type: &str,
) {
    crate::observability::log::slog!(warn, "feishu_outbound", "reaction_failed", http_status = %http_status, body, message_id, emoji_type,);
}

pub(crate) fn log_feishu_reaction_request_failed(error: &impl std::fmt::Display, message_id: &str) {
    crate::observability::log::slog!(warn, "feishu_outbound", "reaction_request_failed", error = %error, message_id,);
}

pub(crate) fn log_feishu_decode_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "feishu_ws", "decode_failed", error = %error,);
}

pub(crate) fn log_feishu_event_received(msg_id: &str, trace_id: &str, event_type: &str) {
    crate::observability::log::slog!(info, "feishu_ws", "event_received", msg_id, trace_id, event_type = %event_type,);
}

pub(crate) fn log_feishu_endpoint_response(
    code: i64,
    reconnect_count: i64,
    reconnect_interval: i64,
    ping_interval: i64,
) {
    crate::observability::log::slog!(
        debug,
        "feishu_ws",
        "endpoint_response",
        code,
        reconnect_count,
        reconnect_interval,
        ping_interval,
    );
}

pub(crate) fn log_feishu_connecting(url: &str, ping_interval: u64) {
    crate::observability::log::slog!(info, "feishu_ws", "connecting", url = %url, ping_interval,);
}

pub(crate) fn log_feishu_handshake(status: u16, hs_status: &str, hs_msg: &str) {
    crate::observability::log::slog!(debug, "feishu_ws", "handshake", status, hs_status, hs_msg,);
}

pub(crate) fn log_feishu_connected(service_id: i32) {
    crate::observability::log::slog!(info, "feishu_ws", "connected", service_id,);
}

pub(crate) fn log_feishu_heartbeat_timeout(elapsed_secs: u64, timeout_secs: u64) {
    crate::observability::log::slog!(
        warn,
        "feishu_ws",
        "heartbeat_timeout",
        elapsed_secs,
        timeout_secs,
    );
}

pub(crate) fn log_feishu_unexpected_ws_msg(msg_type: &str) {
    crate::observability::log::slog!(warn, "feishu_ws", "unexpected_ws_msg", msg_type = %msg_type,);
}

pub(crate) fn log_feishu_invalid_json(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "feishu_ws", "invalid_json", error = %error,);
}

pub(crate) fn log_feishu_channel_busy(event_type: &str) {
    crate::observability::log::slog!(warn, "feishu_ws", "channel_busy", event_type,);
}

pub(crate) fn log_feishu_channel_full(event_type: &str) {
    crate::observability::log::slog!(warn, "feishu_ws", "channel_full", event_type,);
}

pub(crate) fn log_channel_sent_github_reaction(chat_id: &str, message_id: &str, emoji: &str) {
    crate::observability::log::slog!(
        info,
        "channel",
        "sent",
        channel_type = "github",
        chat_id,
        message_id,
        emoji,
    );
}
