use std::fmt;

use serde::Deserialize;
use serde::Serialize;

/// Core run event kinds with a catch-all for extensibility.
///
/// `kind` is stored and transmitted as a dotted string. The core set has typed
/// variants for pattern matching in replay/resume/audit. Unknown kinds are
/// accepted via `Custom(String)` so that tasks, channels, skills, and future
/// modules can emit new event kinds without modifying this enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum RunEventKind {
    UserInput,
    AssistantOutput,
    ToolCall,
    ToolResult,
    SkillEnter,
    SkillExit,
    TaskChange,
    ChannelMessage,
    Checkpoint,
    RunFinish,
    RunError,
    CompactionSummary,
    Custom(String),
}

impl RunEventKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::UserInput => "user.input",
            Self::AssistantOutput => "assistant.output",
            Self::ToolCall => "tool.call",
            Self::ToolResult => "tool.result",
            Self::SkillEnter => "skill.enter",
            Self::SkillExit => "skill.exit",
            Self::TaskChange => "task.change",
            Self::ChannelMessage => "channel.message",
            Self::Checkpoint => "checkpoint",
            Self::RunFinish => "run.finish",
            Self::RunError => "run.error",
            Self::CompactionSummary => "compaction.summary",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "user.input" => Self::UserInput,
            "assistant.output" => Self::AssistantOutput,
            "tool.call" => Self::ToolCall,
            "tool.result" => Self::ToolResult,
            "skill.enter" => Self::SkillEnter,
            "skill.exit" => Self::SkillExit,
            "task.change" => Self::TaskChange,
            "channel.message" => Self::ChannelMessage,
            "checkpoint" => Self::Checkpoint,
            "run.finish" => Self::RunFinish,
            "run.error" => Self::RunError,
            "compaction.summary" => Self::CompactionSummary,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Check if this kind belongs to a domain prefix (e.g. `"tool"`, `"skill"`).
    pub fn is_domain(&self, domain: &str) -> bool {
        self.as_str().starts_with(domain)
            && self
                .as_str()
                .as_bytes()
                .get(domain.len())
                .map_or(false, |&b| b == b'.')
    }
}

impl fmt::Display for RunEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<RunEventKind> for String {
    fn from(kind: RunEventKind) -> Self {
        kind.as_str().to_string()
    }
}

impl TryFrom<String> for RunEventKind {
    type Error = std::convert::Infallible;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self::parse(&s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub event_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub seq: u32,
    pub kind: RunEventKind,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: String,
}
