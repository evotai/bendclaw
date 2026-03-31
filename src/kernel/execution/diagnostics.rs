pub(crate) fn log_tool_parse_failed(
    tool_name: &str,
    tool_call_id: &str,
    raw_arguments: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        warn,
        "tool",
        "parse_failed",
        tool_name = %tool_name,
        tool_call_id = %tool_call_id,
        raw_arguments = %raw_arguments,
        error = %error,
    );
}

pub(crate) fn log_tool_timed_out(tool: &str, tool_call_id: &str) {
    crate::observability::log::slog!(
        warn,
        "tool",
        "timed_out",
        tool = %tool,
        tool_call_id = %tool_call_id,
    );
}
