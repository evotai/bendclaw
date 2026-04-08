use super::completion::CompletionState;

pub const KNOWN_COMMANDS: &[&str] = &[
    "/help",
    "/status",
    "/history",
    "/resume",
    "/new",
    "/clear",
    "/model",
    "/provider",
    "/plan",
    "/act",
    "/version",
    "/quit",
];

pub fn command_short_description(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some("show help"),
        "status" => Some("show current session and config"),
        "history" => Some("print current transcript"),
        "resume" => Some("resume a session"),
        "new" => Some("start a new session"),
        "clear" => Some("clear conversation"),
        "model" => Some("show or change model"),
        "provider" => Some("show or change provider"),
        "plan" => Some("enter planning mode"),
        "act" => Some("return to normal action mode"),
        "version" => Some("show build info"),
        "quit" => Some("exit bendclaw"),
        _ => None,
    }
}

pub fn command_help(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some(
            "/help [command] - Show help information\n\nUsage:\n  /help\n  /help model\n  /help resume",
        ),
        "status" => Some(
            "/status - Show current provider, model, session, cwd, and provider defaults.",
        ),
        "history" => Some(
            "/history - Print the current session transcript from storage.",
        ),
        "resume" => Some(
            "/resume [session-id] - Resume a previous session.\n\nWithout an argument it opens the session selector. Prefixes are accepted when unambiguous.",
        ),
        "new" => Some(
            "/new - Start a fresh session without deleting stored history.",
        ),
        "clear" => Some(
            "/clear - Start a fresh session after confirmation when the current transcript is non-trivial.",
        ),
        "model" => Some(
            "/model [name] - Show or change the active model for the current provider.\n\nWithout an argument it opens the model selector.",
        ),
        "provider" => Some(
            "/provider [anthropic|openai] - Show or change the active provider.",
        ),
        "plan" => Some(
            "/plan - Enter planning mode. Uses only read-only tools. Use /act to return to normal mode.",
        ),
        "act" => Some(
            "/act - Return to normal execution mode with the full tool set.",
        ),
        "version" => Some(
            "/version - Show build version, git sha, branch, and build timestamp.",
        ),
        "quit" => Some(
            "/quit - Exit Bendclaw.",
        ),
        _ => None,
    }
}

pub fn help_command_completions(partial_lower: &str) -> Vec<String> {
    KNOWN_COMMANDS
        .iter()
        .map(|c| c.trim_start_matches('/'))
        .filter(|name| *name != "exit")
        .filter(|name| name.to_lowercase().starts_with(partial_lower))
        .map(|name| name.to_string())
        .collect()
}

/// Returns `true` when `input` starts with a known slash command.
///
/// Only the first word (up to the first space) is checked against
/// `KNOWN_COMMANDS`, so pasted paths like `/some/path.rs` or
/// `:/foo/bar` are *not* treated as commands.
pub fn is_slash_command(input: &str) -> bool {
    let first_word = input.split_whitespace().next().unwrap_or("");
    KNOWN_COMMANDS.contains(&first_word)
}

pub fn command_arg_completions(cmd: &str, arg_part: &str, state: &CompletionState) -> Vec<String> {
    let partial = arg_part.to_lowercase();
    match cmd {
        "/help" => help_command_completions(&partial),
        "/provider" => ["anthropic", "openai"]
            .into_iter()
            .filter(|name| name.starts_with(&partial))
            .map(|name| name.to_string())
            .collect(),
        "/model" => state
            .models
            .iter()
            .filter(|model| model.to_lowercase().starts_with(&partial))
            .cloned()
            .collect(),
        "/resume" => state
            .session_ids
            .iter()
            .filter(|session_id| session_id.starts_with(arg_part))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}
