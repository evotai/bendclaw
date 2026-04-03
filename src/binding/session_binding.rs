use std::sync::Arc;

use crate::storage::sessions::Session;
use crate::storage::sessions::SessionRepo;
use crate::types::ErrorCode;
use crate::types::Result;

/// Resolve, resume, or create a session. This is the single session owner
/// at the app layer. Kernel modules never touch SessionRepo directly.
pub async fn bind_session(
    session_repo: &Arc<dyn SessionRepo>,
    user_id: &str,
    agent_id: &str,
    session_id: Option<&str>,
    resume: bool,
) -> Result<Session> {
    if let Some(sid) = session_id {
        let existing = session_repo.find_session(user_id, agent_id, sid).await?;
        match existing {
            Some(s) => Ok(s),
            None if resume => Err(ErrorCode::not_found(format!(
                "session '{sid}' not found for resume"
            ))),
            None => {
                let session = new_session(user_id, agent_id, sid);
                session_repo.create_session(&session).await?;
                Ok(session)
            }
        }
    } else if resume {
        let latest = session_repo.find_latest_session(user_id, agent_id).await?;
        latest.ok_or_else(|| ErrorCode::not_found("no session to resume"))
    } else {
        let sid = crate::types::id::new_session_id();
        let session = new_session(user_id, agent_id, &sid);
        session_repo.create_session(&session).await?;
        Ok(session)
    }
}

fn new_session(user_id: &str, agent_id: &str, session_id: &str) -> Session {
    let now = chrono::Utc::now().to_rfc3339();
    Session {
        session_id: session_id.to_string(),
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
        title: String::new(),
        scope: String::new(),
        state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
        created_at: now.clone(),
        updated_at: now,
    }
}
