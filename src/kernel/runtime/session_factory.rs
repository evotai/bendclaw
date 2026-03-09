use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::workspace::OpenResolver;
use crate::kernel::session::workspace::SandboxResolver;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::session::Session;
use crate::kernel::session::SessionResources;
use crate::kernel::skills::repository::DatabendSkillRepositoryFactory;
use crate::kernel::tools::registry::create_session_tools;

impl Runtime {
    pub async fn get_or_create_session(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
    ) -> Result<Arc<Session>> {
        self.require_ready()?;

        if let Some(session) = self.sessions.get(session_id) {
            if session.belongs_to(agent_id, user_id) {
                tracing::info!(
                    log_kind = "server_log",
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
            tracing::error!(
                log_kind = "server_log",
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

        let pool = self.databases.agent_pool(agent_id)?;

        let workspace_dir = self.config.workspace.session_dir(user_id, agent_id, session_id);
        if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
            return Err(ErrorCode::internal(format!(
                "failed to create session workspace: {e}"
            )));
        }

        let storage = Arc::new(AgentStore::new(pool.clone(), self.llm.read().clone()));

        let agent_env = match storage.config_get(agent_id).await? {
            Some(record) => record.env,
            None => std::collections::HashMap::new(),
        };

        let resolver: Arc<dyn crate::kernel::session::workspace::PathResolver> =
            if self.config.workspace.sandbox {
                Arc::new(SandboxResolver)
            } else {
                Arc::new(OpenResolver)
            };

        let workspace = Arc::new(Workspace::new(
            workspace_dir,
            self.config.workspace.safe_env_vars.clone(),
            agent_env,
            Duration::from_secs(self.config.workspace.command_timeout_secs),
            self.config.workspace.max_output_bytes,
            resolver,
        ));

        let skill_store_factory =
            Arc::new(DatabendSkillRepositoryFactory::new(self.databases.clone()));
        let tool_registry = Arc::new(create_session_tools(
            storage.clone(),
            self.skills.clone(),
            skill_store_factory,
            pool.clone(),
        ));

        let mut tools = tool_registry.tool_schemas();
        let existing_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.function.name.clone()).collect();
        for skill in self.skills.for_agent(agent_id, user_id) {
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
                llm: Arc::new(RwLock::new(self.llm.read().clone())),
                config: Arc::new(self.config.clone()),
            },
        ));

        self.sessions.insert(session.clone());

        tracing::info!(
            log_kind = "server_log",
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
