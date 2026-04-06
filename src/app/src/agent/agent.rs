use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::conf::LlmConfig;
use crate::error::Result;
use crate::protocol::engine::EngineHandle;
use crate::protocol::engine::EngineOptions;
use crate::protocol::ProtocolEvent;
use crate::protocol::TranscriptItem;

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_duration_secs: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 512,
            max_total_tokens: 100_000_000,
            max_duration_secs: 3600,
        }
    }
}

enum AgentState {
    Live {
        handle: Box<Option<EngineHandle>>,
    },
    Scripted {
        events: Vec<ProtocolEvent>,
        transcripts: Vec<TranscriptItem>,
    },
}

pub struct AppAgent {
    llm: LlmConfig,
    system_prompt: String,
    limits: ExecutionLimits,
    cwd: String,
    state: RwLock<AgentState>,
}

impl AppAgent {
    pub fn new(llm: LlmConfig, cwd: impl Into<String>) -> Self {
        let cwd = cwd.into();
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Self {
            llm,
            system_prompt,
            limits: ExecutionLimits::default(),
            cwd,
            state: RwLock::new(AgentState::Live {
                handle: Box::new(None),
            }),
        }
    }

    pub fn scripted(events: Vec<ProtocolEvent>, transcripts: Vec<TranscriptItem>) -> Arc<Self> {
        Arc::new(Self {
            llm: LlmConfig {
                provider: crate::conf::ProviderKind::Anthropic,
                api_key: String::new(),
                base_url: None,
                model: String::new(),
            },
            system_prompt: String::new(),
            limits: ExecutionLimits::default(),
            cwd: String::new(),
            state: RwLock::new(AgentState::Scripted {
                events,
                transcripts,
            }),
        })
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    pub fn llm(&self) -> &LlmConfig {
        &self.llm
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn limits(&self) -> &ExecutionLimits {
        &self.limits
    }

    pub async fn start(
        &self,
        prompt: String,
        prior_transcripts: &[TranscriptItem],
    ) -> Result<mpsc::UnboundedReceiver<ProtocolEvent>> {
        let mut state = self.state.write().await;
        match &mut *state {
            AgentState::Live { handle } => {
                let options = EngineOptions {
                    provider: self.llm.provider.clone(),
                    model: self.llm.model.clone(),
                    api_key: self.llm.api_key.clone(),
                    base_url: self.llm.base_url.clone(),
                    system_prompt: self.system_prompt.clone(),
                    limits: self.limits.clone(),
                };
                let (rx, engine_handle) =
                    crate::protocol::engine::start_engine(&options, prior_transcripts, prompt)
                        .await?;
                **handle = Some(engine_handle);
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
