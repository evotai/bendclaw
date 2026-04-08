use bendclaw::cli::repl::commands::is_slash_command;
use bendclaw::cli::repl::commands::KNOWN_COMMANDS;
use bendclaw::cli::repl::completion::bare_slash_hint_display;
use bendclaw::cli::repl::completion::is_slash_prefix;

// ---------------------------------------------------------------------------
// is_slash_prefix
// ---------------------------------------------------------------------------

#[test]
fn slash_prefix_bare_slash() {
    assert!(is_slash_prefix("/"));
}

#[test]
fn slash_prefix_valid_commands() {
    assert!(is_slash_prefix("/help"));
    assert!(is_slash_prefix("/status"));
    assert!(is_slash_prefix("/clear"));
    assert!(is_slash_prefix("/quit"));
}

#[test]
fn slash_prefix_partial_command() {
    assert!(is_slash_prefix("/he"));
    assert!(is_slash_prefix("/s"));
    assert!(is_slash_prefix("/cl"));
}

#[test]
fn slash_prefix_with_bang() {
    assert!(is_slash_prefix("/clear!"));
}

#[test]
fn slash_prefix_with_arg() {
    assert!(is_slash_prefix("/help model"));
    assert!(is_slash_prefix("/resume abc123"));
}

#[test]
fn slash_prefix_rejects_file_paths() {
    assert!(!is_slash_prefix("/some/path.rs"));
    assert!(!is_slash_prefix("/usr/local/bin"));
    assert!(!is_slash_prefix("/foo/bar"));
}

#[test]
fn slash_prefix_rejects_paths_with_dots() {
    assert!(!is_slash_prefix("/file.rs"));
    assert!(!is_slash_prefix("/a.b"));
}

#[test]
fn slash_prefix_rejects_paths_with_digits() {
    assert!(!is_slash_prefix("/123"));
    assert!(!is_slash_prefix("/file2"));
}

#[test]
fn slash_prefix_rejects_uppercase() {
    assert!(!is_slash_prefix("/Help"));
    assert!(!is_slash_prefix("/STATUS"));
}

#[test]
fn slash_prefix_rejects_no_slash() {
    assert!(!is_slash_prefix("help"));
    assert!(!is_slash_prefix(""));
    assert!(!is_slash_prefix(":/foo/bar"));
}

// ---------------------------------------------------------------------------
// is_slash_command
// ---------------------------------------------------------------------------

#[test]
fn slash_command_known_commands() {
    for cmd in KNOWN_COMMANDS {
        assert!(is_slash_command(cmd), "expected {cmd} to be recognized");
    }
}

#[test]
fn slash_command_with_args() {
    assert!(is_slash_command("/help model"));
    assert!(is_slash_command("/resume abc123"));
    assert!(is_slash_command("/model claude-3"));
}

#[test]
fn slash_command_rejects_unknown() {
    assert!(!is_slash_command("/unknown"));
    assert!(!is_slash_command("/foo"));
}

#[test]
fn slash_command_rejects_pasted_paths() {
    assert!(!is_slash_command("/some/path.rs"));
    assert!(!is_slash_command("/usr/local/bin"));
    assert!(!is_slash_command("look at :/foo/bar.rs"));
}

#[test]
fn slash_command_rejects_empty() {
    assert!(!is_slash_command(""));
    assert!(!is_slash_command("/"));
    assert!(!is_slash_command("  "));
}

// ---------------------------------------------------------------------------
// bare_slash_hint_display
// ---------------------------------------------------------------------------

#[test]
fn bare_slash_hint_contains_all_commands() {
    let display = bare_slash_hint_display();
    for cmd in KNOWN_COMMANDS {
        let name = &cmd[1..];
        assert!(
            display.contains(name),
            "bare slash hint should contain '{name}', got: {display}"
        );
    }
}

#[test]
fn bare_slash_hint_is_bracketed() {
    let display = bare_slash_hint_display();
    assert!(display.contains('['), "hint should start with '['");
    assert!(display.contains(']'), "hint should end with ']'");
}
