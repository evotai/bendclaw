use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::build_agent_options;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;

#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run_query(
        &self,
        options: AgentRunOptions,
    ) -> Result<mpsc::Receiver<bend_agent::SDKMessage>>;

    async fn take_messages(&self) -> Vec<bend_agent::Message>;

    async fn close(&self);
}

pub struct AgentRunOptions {
    pub llm: LlmConfig,
    pub cwd: String,
    pub messages: Vec<bend_agent::Message>,
    pub prompt: String,
}

pub struct BendAgentRunner {
    agent: tokio::sync::Mutex<Option<bend_agent::Agent>>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Default for BendAgentRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl BendAgentRunner {
    pub fn new() -> Self {
        Self {
            agent: tokio::sync::Mutex::new(None),
            handle: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl AgentRunner for BendAgentRunner {
    async fn run_query(
        &self,
        options: AgentRunOptions,
    ) -> Result<mpsc::Receiver<bend_agent::SDKMessage>> {
        let agent_options = build_agent_options(&options.llm, Some(options.cwd), None);

        let mut agent = bend_agent::Agent::new(agent_options)
            .await
            .map_err(BendclawError::Agent)?;

        agent.messages = options.messages;

        let (rx, handle) = agent.query(&options.prompt).await;

        *self.agent.lock().await = Some(agent);
        *self.handle.lock().await = Some(handle);

        Ok(rx)
    }

    async fn take_messages(&self) -> Vec<bend_agent::Message> {
        if let Some(handle) = self.handle.lock().await.take() {
            let _ = handle.await;
        }
        self.agent
            .lock()
            .await
            .as_ref()
            .map(|a| a.get_messages().to_vec())
            .unwrap_or_default()
    }

    async fn close(&self) {
        if let Some(agent) = self.agent.lock().await.as_ref() {
            agent.close().await;
        }
    }
}
