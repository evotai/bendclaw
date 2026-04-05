use crate::error::Result;
use crate::session::SessionState;
use crate::storage::model::ListTranscriptEntries;
use crate::storage::model::SessionMeta;
use crate::storage::Storage;

pub async fn new_session(
    session_id: String,
    cwd: String,
    model: String,
    storage: &dyn Storage,
) -> Result<SessionState> {
    let meta = SessionMeta::new(session_id, cwd, model);
    storage.put_session(meta.clone()).await?;
    Ok(SessionState::new(meta, Vec::new()))
}

pub async fn load_session(session_id: &str, storage: &dyn Storage) -> Result<Option<SessionState>> {
    let meta = match storage.get_session(session_id).await? {
        Some(m) => m,
        None => return Ok(None),
    };
    let messages = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: session_id.to_string(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    Ok(Some(SessionState::new(meta, messages)))
}
