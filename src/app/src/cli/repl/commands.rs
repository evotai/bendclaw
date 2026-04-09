use super::completion::CompletionState;

pub const KNOWN_COMMANDS: &[&str] = &["/help", "/resume", "/new", "/model", "/plan", "/act"];

pub fn command_short_description(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some("show help"),
        "resume" => Some("resume a session"),
        "new" => Some("start a new session"),
        "model" => Some("show or change model"),
        "plan" => Some("enter planning mode"),
        "act" => Some("return to normal action mode"),
        _ => None,
    }
}

pub fn command_help(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some(
            "/help [command] - Show help information\n\nUsage:\n  /help\n  /help model\n  /help resume",
        ),
        "resume" => Some(
            "/resume [session-id] - Resume a previous session.\n\nWithout an argument it opens the session selector. Prefixes are accepted when unambiguous.",
        ),
        "new" => Some(
            "/new - Start a fresh session without deleting stored history.",
        ),
        "model" => Some(
            "/model [name] - Show or change the active model for the current provider.\n\nWithout an argument it opens the model selector.",
        ),
        "plan" => Some(
            "/plan - Enter planning mode. Uses only read-only tools. Use /act to return to normal mode.",
        ),
        "act" => Some(
            "/act - Return to normal execution mode with the full tool set.",
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
