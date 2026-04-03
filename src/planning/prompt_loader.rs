//! CloudPromptLoader: fetches cloud prompt dependencies from DB,
//! then delegates to the pure build_prompt() function.
//! Also implements PromptResolver for direct use by sessions.

use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::prompt_contract::PromptResolver;
use super::prompt_model::*;
use super::prompt_renderer::build_prompt;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::cluster::ClusterService;
use crate::kernel::runtime::org::OrgServices;
use crate::planning::prompt_diagnostics;
use crate::tools::definition::tool_definition::ToolDefinition;
use crate::types::Result;

const RECENT_ERRORS_LIMIT: u32 = 5;

pub struct CloudPromptLoader {
    storage: Arc<AgentStore>,
    org: Arc<OrgServices>,

    identity: Option<String>,
    soul: Option<String>,
    runtime: Option<String>,
    cwd: Option<PathBuf>,
    recent_errors: Option<String>,
    tools: Option<Arc<Vec<ToolDefinition>>>,
    variables: Option<Vec<PromptVariable>>,
    cluster_client: Option<Arc<ClusterService>>,
    directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
    cached_config: Option<PromptConfig>,
    memory_enabled: bool,
    memory_recall_budget: usize,
    agent_id: String,
    user_id: String,
    session_id: String,
}

impl CloudPromptLoader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<AgentStore>,
        org: Arc<OrgServices>,
        tools: Arc<Vec<ToolDefinition>>,
        variables: Vec<PromptVariable>,
        cached_config: Option<PromptConfig>,
        cwd: PathBuf,
        cluster_client: Option<Arc<ClusterService>>,
        directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
        memory_enabled: bool,
        memory_recall_budget: usize,
        agent_id: String,
        user_id: String,
        session_id: String,
    ) -> Self {
        Self {
            storage,
            org,
            identity: None,
            soul: None,
            runtime: None,
            cwd: Some(cwd),
            recent_errors: None,
            tools: Some(tools),
            variables: Some(variables),
            cluster_client,
            directive,
            cached_config,
            memory_enabled,
            memory_recall_budget,
            agent_id,
            user_id,
            session_id,
        }
    }

    /// Build the full system prompt. Fetches data from DB, then delegates to pure build_prompt().
    async fn build_prompt(&self, meta: &PromptRequestMeta) -> Result<String> {
        let directive_prompt = self.directive.as_ref().and_then(|d| d.cached_prompt());

        let (config, errors_text, state, session_record) =
            self.fetch_all(&self.agent_id, &self.session_id).await;

        let config = config?;
        let state = state?;
        let session_record = session_record?;

        let resolved_config = self.cached_config.clone().or(config);
        let resolved_config = resolved_config
            .map(|mut c| {
                if let Some(ref id) = self.identity {
                    c.identity = id.clone();
                }
                if let Some(ref s) = self.soul {
                    c.soul = s.clone();
                }
                c
            })
            .or_else(|| {
                if self.identity.is_some() || self.soul.is_some() {
                    Some(PromptConfig {
                        system_prompt: String::new(),
                        identity: self.identity.clone().unwrap_or_default(),
                        soul: self.soul.clone().unwrap_or_default(),
                        token_limit_total: None,
                        token_limit_daily: None,
                    })
                } else {
                    None
                }
            });

        let variables = if let Some(ref vars) = self.variables {
            vars.clone()
        } else {
            match self.storage.variable_list().await {
                Ok(r) => r.into_iter().map(Into::into).collect(),
                Err(e) => {
                    prompt_diagnostics::log_prompt_variables_db_failed(&e);
                    vec![]
                }
            }
        };

        let skill_prompts: Vec<SkillPromptEntry> = self
            .org
            .catalog()
            .visible_skills(&self.user_id)
            .into_iter()
            .filter(|s| !s.executable)
            .map(|s| SkillPromptEntry {
                display_name: crate::skills::definition::tool_key::format(&s, &self.user_id),
                description: s.description.clone(),
            })
            .collect();

        let cluster_info = self.build_cluster_info().await;
        let memory_recall = self.build_memory_recall().await;

        let (channel_type, channel_chat_id) = session_record
            .as_ref()
            .and_then(|r| {
                crate::channels::model::context::ChannelContext::from_base_key(&r.base_key)
            })
            .map(|c| (Some(c.channel_type), Some(c.chat_id)))
            .unwrap_or((None, None));

        Ok(build_prompt(PromptInputs {
            seed: PromptSeed {
                cached_config: resolved_config,
                variables,
                skill_prompts,
                directive_prompt,
            },
            tools: self.tools.clone().unwrap_or_else(|| Arc::new(vec![])),
            cwd: self.cwd.clone().unwrap_or_else(|| PathBuf::from(".")),
            system_overlay: meta.system_overlay.clone(),
            skill_overlay: meta.skill_overlay.clone(),
            memory_recall,
            cluster_info,
            recent_errors: if self.recent_errors.is_some() {
                self.recent_errors.clone()
            } else {
                Some(errors_text).filter(|s| !s.is_empty())
            },
            session_state: Some(state),
            channel_type,
            channel_chat_id,
            runtime_override: self.runtime.clone(),
        }))
    }

    async fn fetch_all(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> (
        Result<Option<PromptConfig>>,
        String,
        Result<serde_json::Value>,
        Result<Option<crate::storage::dal::session::record::SessionRecord>>,
    ) {
        let config_fut = async {
            if self.cached_config.is_some() {
                Ok(None)
            } else {
                self.storage
                    .config_get(agent_id)
                    .await
                    .map(|c| c.map(Into::into))
            }
        };
        let errors_fut = self.build_errors_text(session_id);
        let state_fut = self.storage.session_get_state(session_id);
        let session_fut = self.storage.session_load(session_id);
        tokio::join!(config_fut, errors_fut, state_fut, session_fut)
    }

    async fn build_cluster_info(&self) -> Option<String> {
        let cluster_service = self.cluster_client.as_ref()?;
        let nodes = cluster_service.cached_peers();
        let mut buf = String::from("## Cluster\n\n");
        buf.push_str("You are part of a distributed cluster. You can dispatch subtasks to peer nodes for parallel execution.\n\n");
        if nodes.is_empty() {
            buf.push_str("No peer nodes currently available.\n\n");
        } else {
            buf.push_str(
                "| Node ID | Endpoint | Load | Status |\n|---------|----------|------|--------|\n",
            );
            for n in &nodes {
                let meta = n.meta();
                let _ = writeln!(
                    buf,
                    "| {} | {} | {}/{} | {} |",
                    n.node_id, n.endpoint, meta.current_load, meta.max_load, meta.status
                );
            }
            buf.push('\n');
        }
        buf.push_str("Tools:\n");
        buf.push_str("- `cluster_nodes`: Refresh the list of available peer nodes\n");
        buf.push_str("- `cluster_dispatch(node_id, agent_id, task)`: Send a subtask to a peer node by its node_id\n");
        buf.push_str("- `cluster_collect(dispatch_ids, timeout_secs)`: Wait for and collect results from dispatched subtasks\n\n");
        Some(buf)
    }

    async fn build_memory_recall(&self) -> Option<String> {
        let mem = self.org.memory().filter(|_| self.memory_enabled)?;
        let budget = self.memory_recall_budget;
        if budget < 20 {
            return None;
        }
        let fetch_limit = (budget / 80).clamp(5, 50) as u32;
        let entries = mem.recall(&self.user_id, &self.agent_id, fetch_limit).await;
        crate::memory::format::format_for_prompt(&entries, budget)
    }

    async fn build_errors_text(&self, session_id: &str) -> String {
        if let Some(ref s) = self.recent_errors {
            return s.clone();
        }
        match self
            .storage
            .recent_failed_spans(session_id, RECENT_ERRORS_LIMIT)
            .await
        {
            Ok(spans) if !spans.is_empty() => {
                let mut out = String::new();
                for s in &spans {
                    if s.error_message.is_empty() {
                        let _ = writeln!(out, "- `{}`: failed", s.name);
                    } else {
                        let _ = writeln!(out, "- `{}`: {}", s.name, s.error_message);
                    }
                }
                out
            }
            Ok(_) => String::new(),
            Err(e) => {
                prompt_diagnostics::log_prompt_recent_errors_db_failed(&e);
                String::new()
            }
        }
    }
}

#[async_trait]
impl PromptResolver for CloudPromptLoader {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String> {
        self.build_prompt(meta).await
    }
}
