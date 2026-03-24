use std::sync::Arc;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::Message;

pub(crate) struct SessionHistoryLoader {
    storage: Arc<AgentStore>,
}

impl SessionHistoryLoader {
    pub(crate) fn new(storage: Arc<AgentStore>) -> Self {
        Self { storage }
    }

    pub(crate) async fn load(&self, session_id: &str, limit: u32) -> Result<Vec<Message>> {
        let checkpoint = self.storage.run_load_latest_checkpoint(session_id).await?;
        let runs = self.storage.run_list_by_session(session_id, limit).await?;

        let mut seeded = Vec::new();
        let replay = if let Some(checkpoint) = checkpoint {
            if !checkpoint.output.is_empty() {
                seeded.push(Message::compaction(checkpoint.output));
            }

            if checkpoint.checkpoint_through_run_id.is_empty() {
                &runs[..]
            } else if let Some(pos) = runs
                .iter()
                .position(|run| run.id == checkpoint.checkpoint_through_run_id)
            {
                &runs[..pos]
            } else {
                &runs[..]
            }
        } else {
            &runs[..]
        };

        for run in replay.iter().rev() {
            if !run.input.is_empty() {
                seeded.push(Message::user(run.input.clone()).with_run_id(run.id.clone()));
            }
            if !run.output.is_empty() {
                seeded.push(Message::assistant(run.output.clone()).with_run_id(run.id.clone()));
            }
        }

        Ok(seeded)
    }
}
