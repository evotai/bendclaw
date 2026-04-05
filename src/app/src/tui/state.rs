use std::sync::Arc;
use std::time::Instant;

use crate::conf::ProviderKind;
use crate::request::RequestResult;
use crate::storage::model::RunEvent;
use crate::storage::model::SessionMeta;

#[derive(Clone, Copy)]
pub struct CommandHint {
    pub command: &'static str,
    pub summary: &'static str,
}

pub const COMMAND_HINTS: &[CommandHint] = &[
    CommandHint {
        command: "/model",
        summary: "choose model",
    },
    CommandHint {
        command: "/sessions",
        summary: "resume session",
    },
    CommandHint {
        command: "/new",
        summary: "new session",
    },
    CommandHint {
        command: "/clear",
        summary: "clear transcript",
    },
    CommandHint {
        command: "/exit",
        summary: "quit",
    },
];

pub fn matching_command_hints(input: &str) -> Vec<CommandHint> {
    if !input.starts_with('/') {
        return Vec::new();
    }

    let typed = input.trim();
    COMMAND_HINTS
        .iter()
        .filter(|hint| hint.command.starts_with(typed))
        .copied()
        .collect()
}

#[derive(Clone)]
pub struct ModelOption {
    pub provider: ProviderKind,
    pub model: String,
}

impl ModelOption {
    pub fn label(&self) -> String {
        format!("{} / {}", self.provider, self.model)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SessionScope {
    CurrentFolder,
    All,
}

pub enum PopupState {
    Model {
        options: Vec<ModelOption>,
        selected: usize,
        filter: String,
    },
    Session {
        options: Vec<SessionMeta>,
        selected: usize,
        filter: String,
        scope: SessionScope,
    },
}

impl PopupState {
    pub fn push_filter(&mut self, ch: char) {
        match self {
            Self::Model { filter, .. } => filter.push(ch),
            Self::Session { filter, .. } => filter.push(ch),
        }
    }

    pub fn pop_filter(&mut self) {
        match self {
            Self::Model { filter, .. } => {
                filter.pop();
            }
            Self::Session { filter, .. } => {
                filter.pop();
            }
        }
    }

    pub fn select_prev_scope(&mut self) {
        if let Self::Session { scope, .. } = self {
            *scope = SessionScope::CurrentFolder;
        }
    }

    pub fn select_next_scope(&mut self) {
        if let Self::Session { scope, .. } = self {
            *scope = SessionScope::All;
        }
    }

    pub fn reset_selection(&mut self) {
        match self {
            Self::Model { selected, .. } => *selected = 0,
            Self::Session { selected, .. } => *selected = 0,
        }
    }

    pub fn select_prev(&mut self) {
        match self {
            Self::Model { selected, .. } => {
                if *selected > 0 {
                    *selected = selected.saturating_sub(1);
                }
            }
            Self::Session { selected, .. } => {
                if *selected > 0 {
                    *selected = selected.saturating_sub(1);
                }
            }
        }
    }

    pub fn select_next(&mut self, len: usize) {
        match self {
            Self::Model { selected, .. } => {
                if len > 0 && *selected + 1 < len {
                    *selected += 1;
                }
            }
            Self::Session { selected, .. } => {
                if len > 0 && *selected + 1 < len {
                    *selected += 1;
                }
            }
        }
    }
}

pub enum MessageItem {
    Log(String),
    User(String),
    Assistant(String),
    ToolCall {
        title: String,
        lines: Vec<String>,
    },
    ToolResult {
        title: String,
        lines: Vec<String>,
        ok: bool,
    },
    Error(String),
}

pub struct TuiState {
    pub cwd: String,
    pub session_id: Option<String>,
    pub model: ModelOption,
    pub input: String,
    pub messages: Vec<MessageItem>,
    pub streaming_assistant: Option<String>,
    pub popup: Option<PopupState>,
    pub loading: bool,
    pub spinner_index: usize,
    pub session_started_at: Instant,
    pub request_started_at: Option<Instant>,
}

impl TuiState {
    pub fn new(cwd: String, session_id: Option<String>, model: ModelOption) -> Self {
        Self {
            cwd,
            session_id,
            model,
            input: String::new(),
            messages: Vec::new(),
            streaming_assistant: None,
            popup: None,
            loading: false,
            spinner_index: 0,
            session_started_at: Instant::now(),
            request_started_at: None,
        }
    }
}

pub enum TuiEvent {
    RunEvent(Arc<RunEvent>),
    RequestFinished(crate::error::Result<RequestResult>),
}
