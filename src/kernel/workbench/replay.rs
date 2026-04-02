//! Session replay: load raw facts and project a structured summary.

use serde::Deserialize;
use serde::Serialize;

use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::event::Event;
use crate::kernel::workbench::sem_event::SemEvent;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::types::Result;

// ── Output types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReplaySummary {
    pub session_id: String,
    pub runs: Vec<RunSummary>,
    pub tool_timeline: Vec<ToolTimelineEntry>,
    pub capabilities_by_run: Vec<RunCapabilities>,
    pub outcome: OutcomeSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub status: String,
    pub stop_reason: String,
    pub iterations: u32,
    pub duration_ms: u64,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTimelineEntry {
    pub run_id: String,
    pub seq: u32,
    pub tool_call_id: String,
    pub name: String,
    pub success: bool,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCapabilities {
    pub run_id: String,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeSummary {
    pub final_status: String,
    pub final_stop_reason: String,
    pub error: Option<String>,
}

// ── Raw facts loaded from storage ───────────────────────────────────────

pub struct ReplayFacts {
    pub runs: Vec<RunRecord>,
    pub events: Vec<RunEventRecord>,
}

// ── IO: load facts ──────────────────────────────────────────────────────

/// Load all raw facts needed to project a replay for the given session.
/// Events are loaded in a single batch query scoped to the loaded run_ids,
/// so runs and events always cover exactly the same set.
pub async fn load_replay_facts(store: &AgentStore, session_id: &str) -> Result<ReplayFacts> {
    let runs = store.run_list_by_session(session_id, 500).await?;
    let run_ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
    let events = store.run_events_list_by_runs(&run_ids, 50_000).await?;
    Ok(ReplayFacts { runs, events })
}

// ── Pure logic: project replay ──────────────────────────────────────────

/// Project a `SessionReplaySummary` from raw facts. Pure function, no IO.
pub fn project_replay(session_id: &str, facts: ReplayFacts) -> SessionReplaySummary {
    // Runs come DESC from storage — reverse to chronological order.
    let mut runs = facts.runs;
    runs.reverse();

    let run_summaries: Vec<RunSummary> = runs
        .iter()
        .map(|r| {
            let metrics = r.parse_metrics().unwrap_or_default();
            RunSummary {
                run_id: r.id.clone(),
                status: r.status.clone(),
                stop_reason: r.stop_reason.clone(),
                iterations: r.iterations,
                duration_ms: metrics.duration_ms,
                error: r.error.clone(),
            }
        })
        .collect();

    let mut tool_timeline: Vec<ToolTimelineEntry> = Vec::new();
    let mut capabilities_by_run: Vec<RunCapabilities> = Vec::new();

    for record in &facts.events {
        let event: Event = match serde_json::from_str(&record.payload) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            Event::ToolStart {
                tool_call_id, name, ..
            } => {
                tool_timeline.push(ToolTimelineEntry {
                    run_id: record.run_id.clone(),
                    seq: record.seq,
                    tool_call_id,
                    name,
                    success: false,
                    duration_ms: None,
                });
            }
            Event::ToolEnd {
                tool_call_id,
                success,
                operation,
                ..
            } => {
                if let Some(entry) = tool_timeline
                    .iter_mut()
                    .rev()
                    .find(|e| e.run_id == record.run_id && e.tool_call_id == tool_call_id)
                {
                    entry.success = success;
                    entry.duration_ms = Some(operation.duration_ms);
                }
            }
            Event::Semantic(SemEvent::CapabilitiesSnapshot { tools, skills }) => {
                capabilities_by_run.push(RunCapabilities {
                    run_id: record.run_id.clone(),
                    tools,
                    skills,
                });
            }
            _ => {}
        }
    }

    // Derive outcome from the last run (chronological order).
    let outcome = runs
        .last()
        .map(|r| {
            let err = if r.error.is_empty() {
                None
            } else {
                Some(r.error.clone())
            };
            OutcomeSummary {
                final_status: r.status.clone(),
                final_stop_reason: r.stop_reason.clone(),
                error: err,
            }
        })
        .unwrap_or(OutcomeSummary {
            final_status: String::new(),
            final_stop_reason: String::new(),
            error: None,
        });

    SessionReplaySummary {
        session_id: session_id.to_string(),
        runs: run_summaries,
        tool_timeline,
        capabilities_by_run,
        outcome,
    }
}
