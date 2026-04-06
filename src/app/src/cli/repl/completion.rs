use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

use rustyline::completion::Completer;
use rustyline::completion::Pair;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;

use super::commands::command_arg_completions;
use super::commands::command_short_description;
use super::commands::KNOWN_COMMANDS;
use super::render::DIM;
use super::render::RESET;

pub type CompletionStateRef = Arc<RwLock<CompletionState>>;

#[derive(Default)]
pub struct CompletionState {
    pub models: Vec<String>,
    pub session_ids: Vec<String>,
}

pub struct ReplHelper {
    state: CompletionStateRef,
}

impl ReplHelper {
    pub fn new(state: CompletionStateRef) -> Self {
        Self { state }
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];

        if prefix.starts_with('/') && !prefix.contains(' ') {
            let matches: Vec<Pair> = KNOWN_COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| {
                    let cmd_name = &cmd[1..];
                    let desc = command_short_description(cmd_name).unwrap_or("");
                    if desc.is_empty() {
                        Pair {
                            display: cmd.to_string(),
                            replacement: cmd.to_string(),
                        }
                    } else {
                        Pair {
                            display: format!("{cmd:<12} {desc}"),
                            replacement: cmd.to_string(),
                        }
                    }
                })
                .collect();
            return Ok((0, matches));
        }

        if prefix.starts_with('/') {
            if let Some(space_pos) = prefix.find(' ') {
                let cmd = &prefix[..space_pos];
                let arg_part = &prefix[space_pos + 1..];
                if !arg_part.contains(' ') {
                    let state = self.state.read().map_err(|_| {
                        ReadlineError::Io(std::io::Error::other("completion state lock poisoned"))
                    })?;
                    let candidates = command_arg_completions(cmd, arg_part, &state);
                    if !candidates.is_empty() {
                        let pairs = candidates
                            .into_iter()
                            .map(|candidate| Pair {
                                display: candidate.clone(),
                                replacement: candidate,
                            })
                            .collect();
                        return Ok((space_pos + 1, pairs));
                    }
                }
            }
        }

        let word_start = prefix.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let word = &prefix[word_start..];
        if word.is_empty() {
            return Ok((pos, Vec::new()));
        }

        let matches = complete_file_path(word)
            .into_iter()
            .map(|value| Pair {
                display: value.clone(),
                replacement: value,
            })
            .collect();
        Ok((word_start, matches))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if pos != line.len() || !line.starts_with('/') {
            return None;
        }
        let typed = &line[1..];
        if typed.is_empty() || typed.contains(' ') {
            return None;
        }
        for cmd in KNOWN_COMMANDS {
            let cmd_name = &cmd[1..];
            if cmd_name.starts_with(typed) && cmd_name != typed {
                let rest = &cmd_name[typed.len()..];
                if let Some(desc) = command_short_description(cmd_name) {
                    return Some(format!("{rest} - {desc}"));
                }
                return Some(rest.to_string());
            }
        }
        for cmd in KNOWN_COMMANDS {
            let cmd_name = &cmd[1..];
            if cmd_name == typed {
                if let Some(desc) = command_short_description(cmd_name) {
                    return Some(format!(" - {desc}"));
                }
            }
        }
        None
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("{DIM}{hint}{RESET}"))
    }
}

impl Validator for ReplHelper {}
impl rustyline::Helper for ReplHelper {}

pub fn complete_file_path(partial: &str) -> Vec<String> {
    let path = Path::new(partial);

    let (dir, file_prefix) =
        if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
            (partial.to_string(), String::new())
        } else if let Some(parent) = path.parent() {
            let parent_str = if parent.as_os_str().is_empty() {
                ".".to_string()
            } else {
                parent.to_string_lossy().to_string()
            };
            let file_prefix = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            (parent_str, file_prefix)
        } else {
            (".".to_string(), partial.to_string())
        };

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let dir_prefix = if dir == "." && !partial.contains('/') {
        String::new()
    } else if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
        partial.to_string()
    } else {
        let parent = path.parent().unwrap_or(Path::new(""));
        if parent.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", parent.display())
        }
    };

    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&file_prefix) {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|value| value.is_dir())
            .unwrap_or(false);
        let candidate = if is_dir {
            format!("{}{}/", dir_prefix, name)
        } else {
            format!("{}{}", dir_prefix, name)
        };
        matches.push(candidate);
    }
    matches.sort();
    matches
}
