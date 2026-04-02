//! CloudPromptLoader: fetches cloud prompt dependencies from DB,
//! then delegates to the pure build_prompt() function.

use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;

use super::build::build_prompt;
use super::model::*;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::cluster::ClusterService;
use crate::kernel::memory::MemoryService;
use crate::kernel::run::prompt_diagnostics;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::llm::tool::ToolSchema;

const RECENT_ERRORS_LIMIT: u32 = 5;

pub struct CloudPromptLoader {
    storage: Arc<AgentStore>,
    skills: Arc<SkillCatalog>,

    identity: Option<String>,
    soul: Option<String>,
    runtime: Option<String>,
    cwd: Option<PathBuf>,
    recent_errors: Option<String>,
    tools: Option<Arc<Vec<ToolSchema>>>,
    variables: Option<Vec<PromptVariable>>,
    cluster_client: Option<Arc<ClusterService>>,
    directive_prompt: Option<String>,
    cached_config: Option<PromptConfig>,
    memory_service: Option<Arc<MemoryService>>,
    memory_recall_budget: usize,
    system_overlay: Option<String>,
    skill_overlay: Option<String>,
}

impl CloudPromptLoader {
    pub fn new(storage: Arc<AgentStore>, skills: Arc<SkillCatalog>) -> Self {
        Self {
            storage,
            skills,
            identity: None,
            soul: None,
            runtime: None,
            cwd: None,
            recent_errors: None,
            tools: None,
            variables: None,
            cluster_client: None,
            directive_prompt: None,
            cached_config: None,
            memory_service: None,
            memory_recall_budget: 2000,
            system_overlay: None,
            skill_overlay: None,
        }
    }

    pub fn with_cached_config(mut self, config: Option<PromptConfig>) -> Self {
        self.cached_config = config;
        self
    }
    pub fn with_identity(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.identity = Some(s);
        }
        self
    }
    pub fn with_soul(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.soul = Some(s);
        }
        self
    }
    pub fn with_runtime(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.runtime = Some(s);
        }
        self
    }
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }
    pub fn with_recent_errors(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.recent_errors = Some(s);
        }
        self
    }
    pub fn with_tools(mut self, tools: Arc<Vec<ToolSchema>>) -> Self {
        self.tools = Some(tools);
        self
    }
    pub fn with_variables(mut self, vars: Vec<PromptVariable>) -> Self {
        if !vars.is_empty() {
            self.variables = Some(vars);
        }
        self
    }
    pub fn with_cluster_client(mut self, client: Arc<ClusterService>) -> Self {
        self.cluster_client = Some(client);
        self
    }
    pub fn with_directive_prompt(mut self, prompt: Option<String>) -> Self {
        self.directive_prompt = prompt;
        self
    }
    pub fn with_memory_service(
        mut self,
        memory: Option<Arc<MemoryService>>,
        recall_budget: usize,
    ) -> Self {
        self.memory_service = memory;
        self.memory_recall_budget = recall_budget;
        self
    }
    pub fn with_overlays(mut self, system: Option<String>, skill: Option<String>) -> Self {
        self.system_overlay = system;
        self.skill_overlay = skill;
        self
    }

    /// Build the full system prompt. Fetches data from DB, then delegates to pure build_prompt().
    pub async fn build(&self, agent_id: &str, user_id: &str, session_id: &str) -> Result<String> {
        let (config, errors_text, state, session_record) =
            self.fetch_all(agent_id, session_id).await;

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
            .skills
            .visible_skills(user_id)
            .into_iter()
            .filter(|s| !s.executable)
            .map(|s| SkillPromptEntry {
                display_name: crate::kernel::skills::model::tool_key::format(&s, user_id),
                description: s.description.clone(),
            })
            .collect();

        let cluster_info = self.build_cluster_info().await;
        let memory_recall = self.build_memory_recall(user_id, agent_id).await;

        let (channel_type, channel_chat_id) = session_record
            .as_ref()
            .and_then(|r| {
                crate::kernel::channel::context::ChannelContext::from_base_key(&r.base_key)
            })
            .map(|c| (Some(c.channel_type), Some(c.chat_id)))
            .unwrap_or((None, None));

        Ok(build_prompt(PromptInputs {
            seed: PromptSeed {
                cached_config: resolved_config,
                variables,
                skill_prompts,
                directive_prompt: self.directive_prompt.clone(),
            },
            tools: self.tools.clone().unwrap_or_else(|| Arc::new(vec![])),
            cwd: self.cwd.clone().unwrap_or_else(|| PathBuf::from(".")),
            system_overlay: self.system_overlay.clone(),
            skill_overlay: self.skill_overlay.clone(),
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

    async fn build_memory_recall(&self, user_id: &str, agent_id: &str) -> Option<String> {
        let mem = self.memory_service.as_ref()?;
        let budget = self.memory_recall_budget;
        if budget < 20 {
            return None;
        }
        let fetch_limit = (budget / 80).clamp(5, 50) as u32;
        let entries = mem.recall(user_id, agent_id, fetch_limit).await;
        crate::kernel::memory::format::format_for_prompt(&entries, budget)
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
