use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::process::AgentProcess;

/// Session-level state for CLI agent processes.
pub struct CliAgentState {
    session_ids: HashMap<String, String>,
    followup_process: Option<AgentProcess>,
}

impl CliAgentState {
    pub fn new() -> Self {
        Self {
            session_ids: HashMap::new(),
            followup_process: None,
        }
    }

    pub fn get_session_id(&self, agent_type: &str) -> Option<&str> {
        self.session_ids.get(agent_type).map(|s| s.as_str())
    }

    pub fn set_session_id(&mut self, agent_type: &str, session_id: String) {
        self.session_ids.insert(agent_type.to_string(), session_id);
    }

    pub fn take_followup_process(&mut self) -> Option<AgentProcess> {
        self.followup_process.take()
    }

    pub fn set_followup_process(&mut self, process: AgentProcess) {
        self.followup_process = Some(process);
    }

    pub fn has_followup_process(&self) -> bool {
        self.followup_process.is_some()
    }
}

impl Default for CliAgentState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedAgentState = Arc<Mutex<CliAgentState>>;

pub fn new_shared_state() -> SharedAgentState {
    Arc::new(Mutex::new(CliAgentState::new()))
}
