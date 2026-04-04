use chrono::Utc;

use crate::error::Result;
use crate::session::SessionState;
use crate::store::SessionStore;

pub fn update_transcript(state: &mut SessionState, messages: Vec<bend_agent::Message>) {
    state.messages = messages;
    state.meta.turns += 1;
    state.meta.updated_at = Utc::now().to_rfc3339();
}

pub async fn save_transcript(state: &SessionState, store: &dyn SessionStore) -> Result<()> {
    store.save_meta(&state.meta).await?;
    store
        .save_transcript(&state.meta.session_id, &state.messages)
        .await?;
    Ok(())
}
