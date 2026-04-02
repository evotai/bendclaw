use std::sync::Arc;

use crate::kernel::session::diagnostics;
use crate::kernel::session::store::SessionStore;
use crate::kernel::Message;
use crate::types::Result;

pub(crate) struct SessionHistoryLoader {
    storage: Arc<dyn SessionStore>,
}

impl SessionHistoryLoader {
    pub(crate) fn new(storage: Arc<dyn SessionStore>) -> Self {
        Self { storage }
    }

    pub(crate) async fn load(&self, session_id: &str, limit: u32) -> Result<Vec<Message>> {
        let checkpoint = self.storage.run_load_latest_checkpoint(session_id).await?;
        let runs = self.storage.run_list_by_session(session_id, limit).await?;

        let mut seeded = Vec::new();
        let replay = if let Some(ref checkpoint) = checkpoint {
            if !checkpoint.output.is_empty() {
                seeded.push(Message::compaction(checkpoint.output.clone()));
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

        diagnostics::log_history_loaded(
            session_id,
            runs.len(),
            replay.len(),
            seeded.len(),
            &diagnostics::summarize_loaded_history(&seeded, checkpoint.as_ref()),
        );

        Ok(seeded)
    }
}
