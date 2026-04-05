use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;

fn provider_kind(provider: &crate::conf::ProviderKind) -> bend_agent::ProviderKind {
    match provider {
        crate::conf::ProviderKind::Anthropic => bend_agent::ProviderKind::Anthropic,
        crate::conf::ProviderKind::OpenAi => bend_agent::ProviderKind::OpenAi,
    }
}

fn build_agent_options(
    llm: &LlmConfig,
    cwd: Option<String>,
    session_id: Option<String>,
    max_turns: Option<u32>,
    append_system_prompt: Option<String>,
) -> bend_agent::AgentOptions {
    bend_agent::AgentOptions {
        provider: Some(provider_kind(&llm.provider)),
        model: Some(llm.model.clone()),
        api_key: Some(llm.api_key.clone()),
        base_url: llm.base_url.clone(),
        cwd,
        session_id,
        max_turns,
        append_system_prompt,
        ..Default::default()
    }
}

pub struct RequestOptions {
    pub llm: LlmConfig,
    pub cwd: String,
    pub session_id: String,
    pub messages: Vec<bend_agent::Message>,
    pub prompt: String,
    pub max_turns: Option<u32>,
    pub append_system_prompt: Option<String>,
}

enum RunnerState {
    Agent(Box<AgentState>),
    Scripted {
        messages_to_send: Vec<bend_agent::SDKMessage>,
        final_messages: Vec<bend_agent::Message>,
        closed: bool,
    },
}

struct AgentState {
    agent: Option<bend_agent::Agent>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

pub struct RequestRunner {
    state: RwLock<RunnerState>,
}

impl Default for RequestRunner {
    fn default() -> Self {
        Self {
            state: RwLock::new(RunnerState::Agent(Box::new(AgentState {
                agent: None,
                handle: None,
            }))),
        }
    }
}

impl RequestRunner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn scripted(
        messages_to_send: Vec<bend_agent::SDKMessage>,
        final_messages: Vec<bend_agent::Message>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RwLock::new(RunnerState::Scripted {
                messages_to_send,
                final_messages,
                closed: false,
            }),
        })
    }

    pub async fn run_query(
        &self,
        options: RequestOptions,
    ) -> Result<mpsc::Receiver<bend_agent::SDKMessage>> {
        let mut state = self.state.write().await;
        match &mut *state {
            RunnerState::Agent(state) => {
                let agent_options = build_agent_options(
                    &options.llm,
                    Some(options.cwd),
                    Some(options.session_id),
                    options.max_turns,
                    options.append_system_prompt,
                );

                let mut next_agent = bend_agent::Agent::new(agent_options)
                    .await
                    .map_err(BendclawError::Agent)?;
                next_agent.messages = options.messages;

                let (rx, next_handle) = next_agent.query(&options.prompt).await;
                state.agent = Some(next_agent);
                state.handle = Some(next_handle);
                Ok(rx)
            }
            RunnerState::Scripted {
                messages_to_send, ..
            } => {
                let scripted_messages = messages_to_send.clone();
                let (tx, rx) = mpsc::channel(100);
                tokio::spawn(async move {
                    for message in scripted_messages {
                        let _ = tx.send(message).await;
                    }
                });
                Ok(rx)
            }
        }
    }

    pub async fn take_messages(&self) -> Vec<bend_agent::Message> {
        let handle = {
            let mut state = self.state.write().await;
            match &mut *state {
                RunnerState::Agent(state) => state.handle.take(),
                RunnerState::Scripted { final_messages, .. } => return final_messages.clone(),
            }
        };

        if let Some(handle) = handle {
            let _ = handle.await;
        }

        let state = self.state.read().await;
        match &*state {
            RunnerState::Agent(state) => state
                .agent
                .as_ref()
                .map(|value| value.get_messages().to_vec())
                .unwrap_or_default(),
            RunnerState::Scripted { .. } => Vec::new(),
        }
    }

    pub async fn close(&self) {
        let mut state = self.state.write().await;
        match &mut *state {
            RunnerState::Agent(state) => {
                if let Some(agent) = state.agent.as_ref() {
                    agent.close().await;
                }
            }
            RunnerState::Scripted { closed, .. } => {
                *closed = true;
            }
        }
    }
}
