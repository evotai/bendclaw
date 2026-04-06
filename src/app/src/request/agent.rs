use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::protocol::engine::EngineHandle;
use crate::protocol::engine::EngineOptions;
use crate::protocol::model::run::ProtocolEvent;
use crate::protocol::model::transcript::TranscriptItem;
use crate::request::RequestOptions;

enum AgentState {
    Live {
        handle: Option<EngineHandle>,
    },
    Scripted {
        events: Vec<ProtocolEvent>,
        transcripts: Vec<TranscriptItem>,
    },
}

pub struct RequestAgent {
    state: RwLock<AgentState>,
}

impl Default for RequestAgent {
    fn default() -> Self {
        Self {
            state: RwLock::new(AgentState::Live { handle: None }),
        }
    }
}

impl RequestAgent {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn scripted(events: Vec<ProtocolEvent>, transcripts: Vec<TranscriptItem>) -> Arc<Self> {
        Arc::new(Self {
            state: RwLock::new(AgentState::Scripted {
                events,
                transcripts,
            }),
        })
    }

    pub async fn start(
        &self,
        options: RequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<ProtocolEvent>> {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { handle } => {
                let engine_options = EngineOptions {
                    provider: options.llm.provider.clone(),
                    model: options.llm.model.clone(),
                    api_key: options.llm.api_key.clone(),
                    base_url: options.llm.base_url.clone(),
                    cwd: options.cwd.clone(),
                    append_system_prompt: options.append_system_prompt.clone(),
                    max_turns: options.max_turns,
                };
                let (rx, engine_handle) = crate::protocol::engine::start_engine(
                    &engine_options,
                    &options.transcript,
                    options.prompt.clone(),
                )
                .await?;
                *handle = Some(engine_handle);
                Ok(rx)
            }
            AgentState::Scripted { events, .. } => {
                let events = events.clone();
                let (tx, rx) = mpsc::unbounded_channel();
                tokio::spawn(async move {
                    for event in events {
                        let _ = tx.send(event);
                    }
                });
                Ok(rx)
            }
        }
    }

    pub async fn take_transcripts(&self) -> Vec<TranscriptItem> {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { handle } => {
                if let Some(h) = handle.as_mut() {
                    return h.take_transcripts().await;
                }
                Vec::new()
            }
            AgentState::Scripted { transcripts, .. } => transcripts.clone(),
        }
    }

    pub async fn close(&self) {
        let state = self.state.read().await;
        match &*state {
            AgentState::Live { handle } => {
                if let Some(h) = handle.as_ref() {
                    h.abort();
                }
            }
            AgentState::Scripted { .. } => {}
        }
    }
}
