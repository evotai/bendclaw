use crate::error::Result;
use crate::session::SessionMeta;
use crate::session::SessionState;
use crate::store::SessionStore;

pub async fn new_session(
    session_id: String,
    cwd: String,
    model: String,
    store: &dyn SessionStore,
) -> Result<SessionState> {
    let meta = SessionMeta::new(session_id, cwd, model);
    store.save_meta(&meta).await?;
    Ok(SessionState::new(meta, Vec::new()))
}

pub async fn load_session(
    session_id: &str,
    store: &dyn SessionStore,
) -> Result<Option<SessionState>> {
    let meta = match store.load_meta(session_id).await? {
        Some(m) => m,
        None => return Ok(None),
    };
    let messages = store.load_transcript(session_id).await?.unwrap_or_default();
    Ok(Some(SessionState::new(meta, messages)))
}
