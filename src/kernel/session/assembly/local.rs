//! Local session assembly — builds SessionAssembly without cloud dependencies.
//! Contains LocalRuntimeDeps (minimal local dependency set) and build_local_assembly.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use super::backend_factory;
use super::common;
use super::contract::*;
use super::infra_factory;
use super::prompt_factory;
use crate::base::Result;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::session_org::LocalOrgServices;
use crate::kernel::session::store::json::JsonSessionStore;
use crate::kernel::tools::catalog::build_local_toolset;
use crate::kernel::tools::tool_services::NoopSecretUsageSink;
use crate::kernel::trace::TraceWriter;
use crate::kernel::writer::BackgroundWriter;
use crate::llm::provider::LLMProvider;

type ToolWriter = BackgroundWriter<crate::kernel::writer::tool_op::ToolWriteOp>;

// ── LocalRuntimeDeps ────────────────────────────────────────────────

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

// ── LocalBuildOptions ───────────────────────────────────────────────

pub struct LocalBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn LLMProvider>>,
}

// ── build_local_assembly ────────────────────────────────────────────

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

    let workspace =
        common::build_workspace_from_dir(&deps.config, workspace_dir, opts.cwd.as_deref(), &[])?;

    let store = Arc::new(JsonSessionStore::new(session_root));

    let llm = opts
        .llm_override
        .map(|o| Arc::new(RwLock::new(o)) as Arc<RwLock<Arc<dyn LLMProvider>>>)
        .unwrap_or_else(|| deps.llm.clone());

    let secret_sink: Arc<dyn crate::kernel::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let toolset = build_local_toolset(opts.tool_filter, secret_sink);

    let prompt_resolver = prompt_factory::build_local_prompt_resolver(
        toolset.tools.clone(),
        workspace.cwd().to_path_buf(),
    );

    let persistent = backend_factory::build_local_backend(
        store.clone(),
        deps.persist_writer.clone(),
        session_id,
    );

    let infra = infra_factory::build_local_infra(
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
            skill_executor: Arc::new(crate::kernel::skills::runtime::NoopSkillExecutor),
            memory_recaller: None,
        },
    })
}
