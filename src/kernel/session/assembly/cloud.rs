use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;

use super::common;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::resolver::CloudPromptResolver;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptVariable;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::assembly::contract::AgentContext;
use crate::kernel::session::assembly::contract::RunLabels;
use crate::kernel::session::assembly::contract::RuntimeInfra;
use crate::kernel::session::assembly::contract::SessionAssembly;
use crate::kernel::session::assembly::contract::SessionCore;
use crate::kernel::session::assembly::contract::SessionOwner;
use crate::kernel::tools::builtin::catalog::build_cloud_toolset;
use crate::kernel::tools::builtin::catalog::CloudToolsetDeps;
use crate::kernel::tools::execution::tool_services::DbSecretUsageSink;

/// Assembles a full session with cloud config, all tools, skills, memory.
pub struct CloudAssembler {
    pub runtime: Arc<Runtime>,
}

impl CloudAssembler {
    pub async fn assemble(
        &self,
        session_id: &str,
        owner: &SessionOwner,
        opts: CloudBuildOptions,
    ) -> Result<SessionAssembly> {
        let agent_id = &owner.agent_id;
        let user_id = &owner.user_id;
        let pool = self.runtime.databases.agent_pool(agent_id)?;

        // LLM + config
        let (agent_llm, cached_config) = match opts.llm_override {
            Some(llm) => (llm, None),
            None => {
                self.runtime
                    .resolve_agent_llm_and_config(agent_id, &pool)
                    .await?
            }
        };

        // Variables
        let variables = self
            .runtime
            .org
            .variables()
            .list_active(user_id)
            .await
            .map_err(|e| {
                crate::base::ErrorCode::internal(format!("failed to load variables: {e}"))
            })?;
        let variables: Vec<_> = {
            let mut seen = std::collections::HashSet::new();
            variables
                .into_iter()
                .filter(|v| seen.insert(v.key.clone()))
                .collect()
        };
        let prompt_variables: Vec<PromptVariable> =
            variables.iter().map(PromptVariable::from).collect();
        let prompt_config = cached_config.clone().map(PromptConfig::from);

        // Workspace
        let workspace = common::build_workspace(
            &self.runtime.config,
            agent_id,
            session_id,
            user_id,
            opts.cwd.as_deref(),
            &variables,
        )?;

        // Storage
        let storage = Arc::new(AgentStore::new(pool.clone(), agent_llm.clone()));

        // Tools: core + persistent + optional
        let secret_sink: Arc<dyn crate::kernel::tools::execution::tool_services::SecretUsageSink> =
            Arc::new(DbSecretUsageSink::new(pool.clone()));
        let cluster_ref = self.runtime.cluster.read().clone();
        let memory_ref = self.runtime.org.memory().cloned();
        let cluster_deps = cluster_ref.as_ref().map(|svc| {
            let dt = svc.create_dispatch_table();
            (svc.clone(), dt)
        });
        let toolset = build_cloud_toolset(
            CloudToolsetDeps {
                org: self.runtime.org.clone(),
                databend_pool: pool.clone(),
                channels: self.runtime.channels.clone(),
                node_id: self.runtime.config.node_id.clone(),
                cluster: cluster_deps,
                memory: memory_ref,
                secret_sink,
                user_id: user_id.to_string(),
            },
            opts.tool_filter,
        );

        let prompt_resolver = Arc::new(CloudPromptResolver::new(
            storage.clone(),
            self.runtime.org.clone(),
            toolset.tools.clone(),
            prompt_variables.clone(),
            prompt_config.clone(),
            workspace.cwd().to_path_buf(),
            cluster_ref.clone(),
            self.runtime.directive.read().clone(),
            self.runtime.config.memory.recall,
            self.runtime.config.memory.recall_budget,
            agent_id.to_string(),
            user_id.to_string(),
            session_id.to_string(),
        ));

        // Session store — DbSessionStore for persistence, separate from AgentStore
        let session_store = Arc::new(crate::kernel::session::store::db::DbSessionStore::new(
            pool.clone(),
        ));

        // Backend: PersistentBackend for history loading + run initialization
        let persistent = Arc::new(
            crate::kernel::session::backend::persistent::PersistentBackend::new(
                session_store.clone(),
                self.runtime.persist_writer.clone(),
                session_id,
                agent_id,
                user_id,
                prompt_config.clone(),
            ),
        );

        // Trace factory — needs pool before SkillRunner consumes it
        let trace_factory = Arc::new(crate::kernel::trace::factory::DbTraceFactory {
            trace_repo: Arc::new(crate::storage::dal::trace::repo::TraceRepo::new(
                pool.clone(),
            )),
            span_repo: Arc::new(crate::storage::dal::trace::repo::SpanRepo::new(
                pool.clone(),
            )),
        });

        let skill_executor: Arc<dyn crate::kernel::skills::executor::SkillExecutor> =
            Arc::new(crate::kernel::skills::runner::SkillRunner::new(
                agent_id,
                user_id,
                self.runtime.org.skills().clone(),
                workspace.clone(),
                pool,
            ));

        Ok(SessionAssembly {
            labels: RunLabels {
                agent_id: agent_id.as_str().into(),
                user_id: user_id.as_str().into(),
                session_id: session_id.into(),
            },
            core: SessionCore {
                workspace,
                llm: Arc::new(RwLock::new(agent_llm)),
                toolset,
                prompt_resolver,
                context_provider: persistent.clone(),
                run_initializer: persistent,
            },
            infra: RuntimeInfra {
                store: session_store,
                trace_factory,
                tool_writer: self.runtime.tool_writer.clone(),
                trace_writer: self.runtime.trace_writer.clone(),
                persist_writer: self.runtime.persist_writer.clone(),
            },
            agent: AgentContext {
                org: self.runtime.org.clone(),
                config: Arc::new(self.runtime.config.clone()),
                cluster_client: cluster_ref,
                directive: self.runtime.directive.read().clone(),
                prompt_config,
                prompt_variables,
                skill_executor,
                memory_recaller: None,
            },
        })
    }
}

/// Build options for persistent sessions.
#[derive(Default)]
pub struct CloudBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn crate::llm::provider::LLMProvider>>,
}
