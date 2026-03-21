use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::recall::RecallStore;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::workspace::OpenResolver;
use crate::kernel::session::workspace::SandboxResolver;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::session::Session;
use crate::kernel::session::SessionResources;
use crate::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use crate::kernel::tools::registry::create_session_tools;
use crate::kernel::tools::registry::register_cluster_tools;
use crate::storage::dal::variable::VariableRepo;

impl Runtime {
    pub async fn get_or_create_session(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
    ) -> Result<Arc<Session>> {
        self.require_ready()?;

        if let Some(session) = self.sessions.get(session_id) {
            if !session.belongs_to(agent_id, user_id) {
                tracing::error!(
                    stage = "runtime",
                    action = "get_or_create_session",
                    status = "denied",
                    agent_id,
                    user_id,
                    session_id,
                    "runtime session"
                );
                return Err(ErrorCode::denied(format!(
                    "session '{session_id}' belongs to a different agent/user"
                )));
            }
            if session.is_stale() && !session.is_running() {
                self.sessions.remove(session_id);
                tracing::info!(
                    stage = "runtime",
                    action = "get_or_create_session",
                    status = "recreated",
                    reason = "stale_llm_config",
                    agent_id,
                    user_id,
                    session_id,
                    "runtime session"
                );
            } else {
                tracing::info!(
                    stage = "runtime",
                    action = "get_or_create_session",
                    status = "reused",
                    agent_id,
                    user_id,
                    session_id,
                    "runtime session"
                );
                return Ok(session);
            }
        }

        let pool = self.databases.agent_pool(agent_id)?;

        let workspace_dir = self
            .config
            .workspace
            .session_dir(user_id, agent_id, session_id);
        if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
            return Err(ErrorCode::internal(format!(
                "failed to create session workspace: {e}"
            )));
        }

        // Parallelize the two independent DB queries: agent config + variables.
        let variable_repo = VariableRepo::new(pool.clone());
        let (llm_config_result, vars_result) = tokio::join!(
            self.resolve_agent_llm_and_config(agent_id, &pool),
            variable_repo.list_all_active()
        );

        let (agent_llm, cached_config) = llm_config_result?;
        let variable_records = vars_result
            .map_err(|e| ErrorCode::internal(format!("failed to load variables: {e}")))?;

        let storage = Arc::new(AgentStore::new(pool.clone(), agent_llm.clone()));

        let recall_store = Arc::new(RecallStore::new(pool.clone()));

        let resolver: Arc<dyn crate::kernel::session::workspace::PathResolver> =
            if self.config.workspace.sandbox {
                Arc::new(SandboxResolver)
            } else {
                Arc::new(OpenResolver)
            };

        // cwd: where shell/grep/glob operate by default.
        // sandbox=true  → workspace dir (agent is isolated)
        // sandbox=false → $HOME (agent can navigate the user's filesystem)
        let cwd = if self.config.workspace.sandbox {
            workspace_dir.clone()
        } else {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| workspace_dir.clone())
        };

        let workspace = Arc::new(Workspace::from_variable_records(
            workspace_dir,
            cwd,
            self.config.workspace.safe_env_vars.clone(),
            variable_records.clone(),
            Duration::from_secs(self.config.workspace.command_timeout_secs),
            Duration::from_secs(self.config.workspace.max_command_timeout_secs),
            self.config.workspace.max_output_bytes,
            resolver,
        ));

        let skill_store_factory =
            Arc::new(DatabendSkillRepositoryFactory::new(self.databases.clone()));
        let mut tool_registry = create_session_tools(
            storage.clone(),
            self.skills.clone(),
            skill_store_factory,
            pool.clone(),
            self.channels.clone(),
            self.config.node_id.clone(),
            recall_store.clone(),
        );

        // Conditionally register cluster tools
        if let Some(ref svc) = self.cluster {
            let dt = svc.create_dispatch_table();
            register_cluster_tools(&mut tool_registry, svc.clone(), dt);
        }

        let tool_registry = Arc::new(tool_registry);

        let mut tools = tool_registry.tool_schemas();
        let existing_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.function.name.clone()).collect();
        for skill in self.skills.for_agent(agent_id) {
            if !skill.executable {
                continue;
            }
            if existing_names.contains(&skill.name) {
                continue;
            }
            let params = skill.to_json_schema();
            tools.push(crate::llm::tool::ToolSchema::new(
                &skill.name,
                &skill.description,
                params,
            ));
        }

        let tool_count = tools.len();

        let session = Arc::new(Session::new(
            session_id.to_string(),
            agent_id.into(),
            user_id.into(),
            SessionResources {
                workspace,
                tool_registry,
                skills: self.skills.clone(),
                tools: Arc::new(tools),
                storage,
                llm: Arc::new(RwLock::new(agent_llm)),
                config: Arc::new(self.config.clone()),
                variables: variable_records,
                recall: Some(recall_store),
                cluster_client: self.cluster.clone(),
                directive: self.directive.clone(),
                trace_writer: self.trace_writer.clone(),
                persist_writer: self.persist_writer.clone(),
                tool_writer: self.tool_writer.clone(),
                cached_config,
            },
        ));

        self.sessions.insert(session.clone());

        tracing::info!(
            stage = "runtime",
            action = "get_or_create_session",
            status = "created",
            agent_id,
            user_id,
            session_id,
            workspace_dir = %self.config.workspace.session_dir(user_id, agent_id, session_id).display(),
            tool_count,
            "runtime session"
        );

        Ok(session)
    }
}
