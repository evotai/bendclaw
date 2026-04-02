use serde::Deserialize;
use serde::Serialize;

use crate::kernel::run::event::Event;
use crate::kernel::skills::model::skill::Skill;
use crate::kernel::skills::model::tool_key;
use crate::llm::tool::ToolSchema;

/// Semantic events layered on top of raw runtime facts.
///
/// Carried by `Event::Semantic(SemEvent)` and persisted into `run_events`
/// through the existing persistence path. No new table, no new channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SemEvent {
    /// Snapshot of tools and skills visible at run start.
    ///
    /// Run-level granularity — records the full set assembled by `CloudPromptLoader`,
    /// not the per-turn progressive subset from `ProgressiveToolView`.
    CapabilitiesSnapshot {
        tools: Vec<String>,
        skills: Vec<String>,
    },
}

impl SemEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CapabilitiesSnapshot { .. } => "sem.capabilities_snapshot",
        }
    }
}

/// Build a `CapabilitiesSnapshot` event from the tool schemas and visible skills.
///
/// Skill classification is owned here, not by the caller:
/// - Only non-executable skills are recorded (executable ones are already in `tools`)
/// - Skill names are formatted using `tool_key::format` to match what the model sees
pub fn capture_capabilities(tools: &[ToolSchema], skills: &[Skill], user_id: &str) -> Event {
    let skill_names: Vec<String> = skills
        .iter()
        .filter(|s| !s.executable)
        .map(|s| tool_key::format(s, user_id))
        .collect();
    Event::Semantic(SemEvent::CapabilitiesSnapshot {
        tools: tools.iter().map(|t| t.function.name.clone()).collect(),
        skills: skill_names,
    })
}
