use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunRisk {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRelation {
    Append,
    Revise,
    ForkOrAsk,
}

#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub session_id: String,
    pub run_id: String,
    pub summary: String,
    pub risk: RunRisk,
    pub target_scope: Option<String>,
    pub started_at: Instant,
}

impl RunSnapshot {
    pub fn from_input(session_id: &str, run_id: &str, input: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            summary: input.chars().take(200).collect(),
            risk: RunRisk::ReadOnly,
            target_scope: None,
            started_at: Instant::now(),
        }
    }
}

pub trait TurnRelationClassifier: Send + Sync {
    fn classify(&self, snapshot: &RunSnapshot, new_input: &str) -> TurnRelation;
}

/// Phase 1 stub: always returns ForkOrAsk (fail-safe default).
pub struct StubClassifier;

impl TurnRelationClassifier for StubClassifier {
    fn classify(&self, _snapshot: &RunSnapshot, _new_input: &str) -> TurnRelation {
        TurnRelation::ForkOrAsk
    }
}
