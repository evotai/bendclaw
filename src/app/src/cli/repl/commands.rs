use super::completion::CompletionState;

pub const KNOWN_COMMANDS: &[&str] = &[
    "/help", "/resume", "/new", "/model", "/plan", "/act", "/env", "/log",
];

// ---------------------------------------------------------------------------
// Slash command prefix resolution
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum ResolvedSlashCommand {
    /// Exact or unique-prefix match — contains the normalised full input.
    Resolved(String),
    /// Multiple commands match the prefix.
    Ambiguous(Vec<String>),
    /// No known command matches.
    Unknown,
}

/// Resolve a user input that looks like a slash command.
///
/// Accepts exact matches (`/help`) as well as unique prefixes (`/h`).
/// Arguments after the command token are preserved.
///
/// Inputs that don't look like a slash command (e.g. file paths, plain text)
/// return `Unknown`.
pub fn resolve_slash_command(input: &str) -> ResolvedSlashCommand {
    let input = input.trim();
    let Some(rest) = input.strip_prefix('/') else {
        return ResolvedSlashCommand::Unknown;
    };

    // Extract the command token (before the first space).
    let (cmd_token, args) = match rest.split_once(' ') {
        Some((cmd, args)) => (cmd, Some(args)),
        None => (rest, None),
    };

    // Reject anything that doesn't look like a hand-typed command name:
    // only ASCII lowercase letters (and optionally trailing `!`).
    if cmd_token.is_empty()
        || !cmd_token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'!')
    {
        return ResolvedSlashCommand::Unknown;
    }

    let full_token = format!("/{cmd_token}");

    // Exact match — fast path.
    if KNOWN_COMMANDS.contains(&full_token.as_str()) {
        return ResolvedSlashCommand::Resolved(input.to_string());
    }

    // Prefix match.
    let matches: Vec<&str> = KNOWN_COMMANDS
        .iter()
        .filter(|cmd| cmd.starts_with(&full_token))
        .copied()
        .collect();

    match matches.len() {
        0 => ResolvedSlashCommand::Unknown,
        1 => {
            let resolved_cmd = matches[0];
            match args {
                Some(a) => ResolvedSlashCommand::Resolved(format!("{resolved_cmd} {a}")),
                None => ResolvedSlashCommand::Resolved(resolved_cmd.to_string()),
            }
        }
        _ => ResolvedSlashCommand::Ambiguous(matches.iter().map(|s| s.to_string()).collect()),
    }
}

pub fn command_short_description(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some("show help"),
        "resume" => Some("resume a session"),
        "new" => Some("start a new session"),
        "model" => Some("show or change model"),
        "plan" => Some("enter planning mode"),
        "act" => Some("return to normal action mode"),
        "env" => Some("manage variables"),
        "log" => Some("analyze session log in a side conversation"),
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
            "/model [name|n] - Show or change the active model.\n\nWithout an argument it opens the model selector.\nUse /model n (or /m n) to cycle to the next model.",
        ),
        "plan" => Some(
            "/plan - Enter planning mode. Uses only read-only tools. Use /act to return to normal mode.",
        ),
        "act" => Some(
            "/act - Return to normal execution mode with the full tool set.",
        ),
        "env" => Some(
            "/env - Manage variables.\n\nUsage:\n  /env              List configured variables\n  /env set KEY=VAL  Set a variable\n  /env del KEY      Delete a variable\n  /env load FILE    Import variables from .env file",
        ),
        "log" => Some(
            "/log [question] - Analyze the current session log.\n\nWithout an argument it shows the log file path.\nWith a question it opens a side conversation for log analysis.\nType /done to return to the main session.",
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

/// Hidden commands that are recognised but not shown in `/help` or tab
/// completion.
const HIDDEN_COMMANDS: &[&str] = &[];

/// Returns `true` when `input` starts with a known slash command
/// (including hidden ones).
///
/// Only the first word (up to the first space) is checked against
/// `KNOWN_COMMANDS` and `HIDDEN_COMMANDS`, so pasted paths like
/// `/some/path.rs` or `:/foo/bar` are *not* treated as commands.
pub fn is_slash_command(input: &str) -> bool {
    let first_word = input.split_whitespace().next().unwrap_or("");
    KNOWN_COMMANDS.contains(&first_word) || HIDDEN_COMMANDS.contains(&first_word)
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
