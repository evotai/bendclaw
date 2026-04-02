//! Unified session builder — assembles SessionAssembly for both cloud and local modes.
//!
//! Cloud path: `SessionBuilder::build_cloud()` (async, requires Runtime)
//! Local path: `build_local_assembly()` (sync, minimal deps)

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use super::backend_builder;
use super::infra_builder;
use super::prompt_builder;
use super::session_capabilities::*;
use super::workspace_builder;
use crate::base::Result;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::tools::tool_services::NoopSecretUsageSink;
use crate::kernel::trace::TraceWriter;
use crate::kernel::writer::BackgroundWriter;
use crate::llm::provider::LLMProvider;

type ToolWriter = BackgroundWriter<crate::kernel::writer::tool_op::ToolWriteOp>;

// ═══════════════════════════════════════════════════════════════════
//  Cloud session building
// ═══════════════════════════════════════════════════════════════════

use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::planning::PromptConfig;
use crate::kernel::run::planning::PromptVariable;
use crate::kernel::runtime::Runtime;
use crate::kernel::tools::selection::build_cloud_toolset;
use crate::kernel::tools::selection::CloudToolsetDeps;
use crate::kernel::tools::tool_services::DbSecretUsageSink;

/// Builds a full cloud session with all services, tools, skills, memory.
pub struct SessionBuilder {
    pub runtime: Arc<Runtime>,
}

impl SessionBuilder {
    pub async fn build_cloud(
        &self,
        session_id: &str,
        owner: &SessionOwner,
        opts: CloudBuildOptions,
    ) -> Result<SessionAssembly> {
        let agent_id = &owner.agent_id;
        let user_id = &owner.user_id;
        let pool = self.runtime.databases.agent_pool(agent_id)?;

        let (agent_llm, cached_config) = match opts.llm_override {
            Some(llm) => (llm, None),
            None => {
                self.runtime
                    .resolve_agent_llm_and_config(agent_id, &pool)
                    .await?
            }
        };

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

        let workspace = workspace_builder::build_workspace(
            &self.runtime.config,
            agent_id,
            session_id,
            user_id,
            opts.cwd.as_deref(),
            &variables,
        )?;

        let storage = Arc::new(AgentStore::new(pool.clone(), agent_llm.clone()));

        let secret_sink: Arc<dyn crate::kernel::tools::tool_services::SecretUsageSink> =
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

        let prompt_resolver = prompt_builder::build_cloud_prompt_resolver(
            prompt_builder::CloudPromptResolverConfig {
                storage: storage.clone(),
                org: self.runtime.org.clone(),
                tools: toolset.definitions.clone(),
                variables: prompt_variables.clone(),
                prompt_config: prompt_config.clone(),
                cwd: workspace.cwd().to_path_buf(),
                cluster_client: cluster_ref.clone(),
                directive: self.runtime.directive.read().clone(),
                memory_enabled: self.runtime.config.memory.recall,
                memory_recall_budget: self.runtime.config.memory.recall_budget,
                agent_id: agent_id.to_string(),
                user_id: user_id.to_string(),
                session_id: session_id.to_string(),
            },
        );

        let (session_store, persistent) = backend_builder::build_cloud_backend(
            pool.clone(),
            self.runtime.persist_writer.clone(),
            session_id,
            agent_id,
            user_id,
            prompt_config.clone(),
        );

        let infra = infra_builder::build_cloud_infra(
            session_store,
            pool.clone(),
            self.runtime.tool_writer.clone(),
            self.runtime.trace_writer.clone(),
            self.runtime.persist_writer.clone(),
        );

        let skill_executor: Arc<dyn crate::kernel::run::execution::skills::SkillExecutor> =
            Arc::new(crate::kernel::run::execution::skills::SkillRunner::new(
                agent_id,
                user_id,
                self.runtime.catalog.clone(),
                self.runtime.org.manager().clone(),
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
            infra,
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

/// Build options for cloud sessions.
#[derive(Default)]
pub struct CloudBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn LLMProvider>>,
}

// ═══════════════════════════════════════════════════════════════════
//  Local session building
// ═══════════════════════════════════════════════════════════════════

use crate::kernel::runtime::session_org::LocalOrgServices;
use crate::kernel::session::store::json::JsonSessionStore;
use crate::kernel::tools::selection::build_local_toolset;

/// Minimal dependency set for local (CLI) sessions.
pub struct LocalRuntimeDeps {
    pub config: AgentConfig,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub tool_writer: ToolWriter,
    pub trace_writer: TraceWriter,
    pub persist_writer: PersistWriter,
}

impl LocalRuntimeDeps {
    pub fn new(config: AgentConfig, llm: Arc<dyn LLMProvider>) -> Self {
        Self {
            config,
            llm: Arc::new(RwLock::new(llm)),
            tool_writer: BackgroundWriter::noop("tool_write"),
            trace_writer: TraceWriter::noop(),
            persist_writer: crate::kernel::run::persist_op::spawn_persist_writer(),
        }
    }
}

/// Build options for local sessions.
pub struct LocalBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn LLMProvider>>,
}

/// Assembles a local session without cloud dependencies.
pub fn build_local_assembly(
    deps: &LocalRuntimeDeps,
    session_id: &str,
    opts: LocalBuildOptions,
) -> Result<SessionAssembly> {
    let session_root = PathBuf::from(&deps.config.workspace.root_dir)
        .join("local")
        .join("sessions")
        .join(session_id);
    let workspace_dir = session_root.join("workspace");

    let workspace = workspace_builder::build_workspace_from_dir(
        &deps.config,
        workspace_dir,
        opts.cwd.as_deref(),
        &[],
    )?;

    let store = Arc::new(JsonSessionStore::new(session_root));

    let llm = opts
        .llm_override
        .map(|o| Arc::new(RwLock::new(o)) as Arc<RwLock<Arc<dyn LLMProvider>>>)
        .unwrap_or_else(|| deps.llm.clone());

    let secret_sink: Arc<dyn crate::kernel::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let toolset = build_local_toolset(opts.tool_filter, secret_sink);

    let prompt_resolver = prompt_builder::build_local_prompt_resolver(
        toolset.definitions.clone(),
        workspace.cwd().to_path_buf(),
    );

    let persistent = backend_builder::build_local_backend(
        store.clone(),
        deps.persist_writer.clone(),
        session_id,
    );

    let infra = infra_builder::build_local_infra(
        store,
        deps.tool_writer.clone(),
        deps.trace_writer.clone(),
        deps.persist_writer.clone(),
    );

    Ok(SessionAssembly {
        labels: RunLabels {
            agent_id: "local".into(),
            user_id: "cli".into(),
            session_id: session_id.into(),
        },
        core: SessionCore {
            workspace,
            llm,
            toolset,
            prompt_resolver,
            context_provider: persistent.clone(),
            run_initializer: persistent,
        },
        infra,
        agent: AgentContext {
            org: Arc::new(LocalOrgServices),
            config: Arc::new(deps.config.clone()),
            cluster_client: None,
            directive: None,
            prompt_config: None,
            prompt_variables: vec![],
            skill_executor: Arc::new(crate::kernel::run::execution::skills::NoopSkillExecutor),
            memory_recaller: None,
        },
    })
}
