//! Structured logging macros enforcing `"{stage} {status}"` message format.
//!
//! - `slog!` — core macro, any stage/status combination.
//! - `storage_log!` — domain macro for storage ops (requires `database` + `sql`).
//! - `channel_log!` — domain macro for channel ops (pre-fills `stage="channel"`).
//!
//! All macros support an optional `msg = <expr>` parameter to override the
//! default `"{stage} {status}"` message with a custom human-readable string.
//! The `stage` and `status` structured fields are always emitted for JSON logs.

/// Core structured log macro.
///
/// ```ignore
/// slog!(info, "lease", "claimed", table = "tasks", resource_id = %id,);
/// slog!(info, "lease", "claimed", msg = "custom message", table = "tasks",);
/// ```
macro_rules! slog {
    ($level:ident, $stage:expr, $status:expr, msg = $msg:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            "{}", $msg
        )
    };
    ($level:ident, $stage:expr, $status:expr, msg = $msg:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            "{}", $msg
        )
    };
    ($level:ident, $stage:expr, $status:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            concat!($stage, " ", $status)
        )
    };
    ($level:ident, $stage:expr, $status:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            concat!($stage, " ", $status)
        )
    };
}
pub(crate) use slog;

/// Storage operation log. Pre-fills `stage="storage"`, requires `database` + `sql`.
///
/// ```ignore
/// storage_log!(debug, "exec", "started",
///     database = "my_db", sql = &sql,
///     base_url = %self.base_url,
/// );
/// ```
macro_rules! storage_log {
    ($level:ident, $op:expr, $status:expr,
     database = $db:expr, sql = $sql:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = "storage",
            operation = $op,
            status = $status,
            database = $db,
            sql = %$crate::storage::pool::truncate_sql($sql),
            $($rest)*
            concat!("storage ", $status)
        )
    };
    ($level:ident, $op:expr, $status:expr,
     database = $db:expr, sql = $sql:expr $(,)?) => {
        tracing::$level!(
            stage = "storage",
            operation = $op,
            status = $status,
            database = $db,
            sql = %$crate::storage::pool::truncate_sql($sql),
            concat!("storage ", $status)
        )
    };
}
pub(crate) use storage_log;

/// Channel operation log. Pre-fills `stage="channel"`.
/// Callers should include `channel_type` and `account_id` fields.
///
/// ```ignore
/// channel_log!(info, "inbound", "accepted",
///     channel_type = %account.channel_type,
///     account_id = %account.channel_account_id,
///     chat_id,
/// );
/// ```
macro_rules! channel_log {
    ($level:ident, $op:expr, $status:expr, msg = $msg:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = "channel",
            operation = $op,
            status = $status,
            $($rest)*
            "{}", $msg
        )
    };
    ($level:ident, $op:expr, $status:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = "channel",
            operation = $op,
            status = $status,
            $($rest)*
            concat!("channel ", $status)
        )
    };
    ($level:ident, $op:expr, $status:expr $(,)?) => {
        tracing::$level!(
            stage = "channel",
            operation = $op,
            status = $status,
            concat!("channel ", $status)
        )
    };
}
pub(crate) use channel_log;

/// Run-scoped log. Pre-fills `run_id`, `session_id`, `agent_id`, `turn` from a `ServerCtx`.
///
/// ```ignore
/// run_log!(info, &self.ops_ctx(iteration), "llm", "completed",
///     elapsed_ms = ms, tokens = total, model = %model,
/// );
/// run_log!(info, ctx, "turn", "started",
///     msg = format!("  iter-{}", iteration),
///     tool_strategy = %strategy,
/// );
/// ```
macro_rules! run_log {
    ($level:ident, $ctx:expr, $stage:expr, $status:expr, msg = $msg:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            run_id = $ctx.run_id,
            session_id = $ctx.session_id,
            agent_id = $ctx.agent_id,
            turn = $ctx.turn,
            "{}", $msg
        )
    };
    ($level:ident, $ctx:expr, $stage:expr, $status:expr, msg = $msg:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            run_id = $ctx.run_id,
            session_id = $ctx.session_id,
            agent_id = $ctx.agent_id,
            turn = $ctx.turn,
            "{}", $msg
        )
    };
    ($level:ident, $ctx:expr, $stage:expr, $status:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            run_id = $ctx.run_id,
            session_id = $ctx.session_id,
            agent_id = $ctx.agent_id,
            turn = $ctx.turn,
            concat!($stage, " ", $status)
        )
    };
    ($level:ident, $ctx:expr, $stage:expr, $status:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            run_id = $ctx.run_id,
            session_id = $ctx.session_id,
            agent_id = $ctx.agent_id,
            turn = $ctx.turn,
            concat!($stage, " ", $status)
        )
    };
}
pub(crate) use run_log;
