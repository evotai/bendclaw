//! PersistentBackend<S> — shared backend for both local and cloud sessions.
//!
//! Implements SessionContextProvider (history + token limits) and
//! RunInitializer (sync fire-and-forget via PersistOp).

use std::sync::Arc;

use async_trait::async_trait;

use super::context::SessionContextProvider;
use super::sink::RunInitializer;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::run::persist_op::PersistOp;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::run::planning::PromptConfig;
use crate::kernel::run::usage::UsageScope;
use crate::kernel::session::runtime::history_loader::SessionHistoryLoader;
use crate::kernel::session::store::SessionStore;
use crate::kernel::Message;

/// Persistent backend generic over any SessionStore implementation.
/// Local uses `PersistentBackend<JsonSessionStore>`,
/// cloud uses `PersistentBackend<DbSessionStore>`.
pub struct PersistentBackend<S: SessionStore + 'static> {
    store: Arc<S>,
    persist_writer: PersistWriter,
    session_id: String,
    agent_id: String,
    user_id: String,
    prompt_config: Option<PromptConfig>,
}

impl<S: SessionStore + 'static> PersistentBackend<S> {
    pub fn new(
        store: Arc<S>,
        persist_writer: PersistWriter,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        user_id: impl Into<String>,
        prompt_config: Option<PromptConfig>,
    ) -> Self {
        Self {
            store,
            persist_writer,
            session_id: session_id.into(),
            agent_id: agent_id.into(),
            user_id: user_id.into(),
            prompt_config,
        }
    }
}

#[async_trait]
impl<S: SessionStore + 'static> SessionContextProvider for PersistentBackend<S> {
    async fn load_history(&self, limit: usize) -> Result<Vec<Message>> {
        let loader = SessionHistoryLoader::new(self.store.clone());
        loader.load(&self.session_id, limit as u32).await
    }

    async fn enforce_token_limits(&self) -> Result<()> {
        let config = match &self.prompt_config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };
        let need_total = config.token_limit_total.is_some();
        let need_daily = config.token_limit_daily.is_some();
        if !need_total && !need_daily {
            return Ok(());
        }

        let total_fut = async {
            if need_total {
                Some(
                    self.store
                        .usage_summarize(UsageScope::AgentTotal {
                            agent_id: self.agent_id.clone(),
                        })
                        .await,
                )
            } else {
                None
            }
        };
        let daily_fut = async {
            if need_daily {
                let day = crate::storage::time::now().date_naive().to_string();
                Some(
                    self.store
                        .usage_summarize(UsageScope::AgentDaily {
                            agent_id: self.agent_id.clone(),
                            day,
                        })
                        .await,
                )
            } else {
                None
            }
        };

        let (total_result, daily_result) = tokio::join!(total_fut, daily_fut);

        if let (Some(limit), Some(result)) = (config.token_limit_total, total_result) {
            let used = result?.total_tokens;
            if used >= limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token total limit exceeded: used={used} limit={limit}"
                )));
            }
        }
        if let (Some(limit), Some(result)) = (config.token_limit_daily, daily_result) {
            let used = result?.total_tokens;
            if used >= limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token daily limit exceeded: used={used} limit={limit}"
                )));
            }
        }
        Ok(())
    }
}

impl<S: SessionStore + 'static> RunInitializer for PersistentBackend<S> {
    fn init_run(&self, input: &str, parent_run_id: Option<&str>, node_id: &str) -> Result<String> {
        let run_id = crate::kernel::new_run_id();

        self.persist_writer.send(PersistOp::InitRun {
            storage: self.store.clone(),
            run_id: run_id.clone(),
            session_id: self.session_id.clone(),
            agent_id: self.agent_id.clone(),
            user_id: self.user_id.clone(),
            user_message: input.to_string(),
            parent_run_id: parent_run_id.unwrap_or_default().to_string(),
            node_id: node_id.to_string(),
        });

        Ok(run_id)
    }
}
