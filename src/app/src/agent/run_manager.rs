//! Channel conversation routing — maps external conversation keys to sessions,
//! serializes per-conversation to prevent duplicate runs.
//!
//! Direct session APIs (HTTP, NAPI) bypass this and call Agent directly.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use super::run::run::Run;
use super::Agent;
use super::QueryRequest;
use crate::error::Result;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ConversationKey {
    channel: String,
    key: String,
}

impl ConversationKey {
    pub fn new(channel: &str, scope: &str) -> Self {
        Self {
            channel: channel.to_string(),
            key: format!("{channel}:{scope}"),
        }
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }
}

pub enum SendOutcome {
    Started(Run),
    Steered,
}

struct Conversation {
    session_id: Option<String>,
    gate: Arc<AsyncMutex<()>>,
}

pub struct RunManager {
    agent: Arc<Agent>,
    conversations: SyncMutex<HashMap<ConversationKey, Conversation>>,
}

impl RunManager {
    pub fn new(agent: Arc<Agent>) -> Arc<Self> {
        Arc::new(Self {
            agent,
            conversations: SyncMutex::new(HashMap::new()),
        })
    }

    pub fn agent(&self) -> &Arc<Agent> {
        &self.agent
    }

    pub async fn send(&self, key: &ConversationKey, request: QueryRequest) -> Result<SendOutcome> {
        let gate = {
            let mut convs = self.conversations.lock();
            let conv = convs.entry(key.clone()).or_insert_with(|| Conversation {
                session_id: None,
                gate: Arc::new(AsyncMutex::new(())),
            });
            conv.gate.clone()
        };

        let _guard = gate.lock().await;

        let session_id = self
            .conversations
            .lock()
            .get(key)
            .and_then(|c| c.session_id.clone());

        if let Some(ref sid) = session_id {
            if self.agent.has_active_run(sid) {
                self.agent.steer(sid, request.input.clone());
                return Ok(SendOutcome::Steered);
            }
        }

        let request = request.session_id(session_id).source(key.channel());
        let run = self.agent.query(request).await?;

        self.conversations
            .lock()
            .entry(key.clone())
            .and_modify(|c| c.session_id = Some(run.session_id.clone()));

        Ok(SendOutcome::Started(run))
    }
}
