//! Local session assembly — builds SessionAssembly without cloud dependencies.
//! Contains LocalRuntimeDeps (minimal local dependency set) and build_local_assembly.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use super::common;
use super::contract::*;
use crate::base::Result;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::run::prompt::model::PromptSeed;
use crate::kernel::run::prompt::resolver::LocalPromptResolver;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::session_org::LocalOrgServices;
use crate::kernel::session::store::json::JsonSessionStore;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::services::NoopSecretUsageSink;
use crate::kernel::trace::factory::NoopTraceFactory;
use crate::kernel::trace::TraceWriter;
use crate::kernel::writer::BackgroundWriter;
use crate::llm::provider::LLMProvider;

type ToolWriter = BackgroundWriter<crate::kernel::writer::tool_op::ToolWriteOp>;

// ── LocalRuntimeDeps ────────────────────────────────────────────────

/// Minimal dependency set for local sessions.
/// Does NOT pull in AgentDatabases, SessionLifecycle, or Pool.
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

/// Options for local assembly.
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
    // Local directory layout:
    //   {root_dir}/local/sessions/{session_id}/
    //     workspace/       ← Workspace.dir
    //     session.json     ← JsonSessionStore data
    //     runs/
    //     events/
    //     usage/
    let session_root = PathBuf::from(&deps.config.workspace.root_dir)
        .join("local")
        .join("sessions")
        .join(session_id);
    let workspace_dir = session_root.join("workspace");

    let workspace =
        common::build_workspace_from_dir(&deps.config, workspace_dir, opts.cwd.as_deref(), &[])?;

    // Store at session_root — same level as workspace/
    let store = Arc::new(JsonSessionStore::new(session_root));

    let llm = opts
        .llm_override
        .map(|o| Arc::new(RwLock::new(o)) as Arc<RwLock<Arc<dyn LLMProvider>>>)
        .unwrap_or_else(|| deps.llm.clone());

    let secret_sink: Arc<dyn crate::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let mut registry = ToolRegistry::new();
    crate::kernel::tools::catalog::register_core(&mut registry, secret_sink);
    let registry = Arc::new(registry);

    let mut tools = registry.tool_schemas();
    let allowed_tool_names = common::apply_tool_filter(&mut tools, opts.tool_filter);
    let tools_arc = Arc::new(tools);

    let prompt_resolver = Arc::new(LocalPromptResolver::new(
        PromptSeed::default(),
        tools_arc.clone(),
        workspace.cwd().to_path_buf(),
    ));

    let persistent = Arc::new(
        crate::kernel::session::backend::persistent::PersistentBackend::new(
            store.clone(),
            deps.persist_writer.clone(),
            session_id,
            "local",
            "cli",
            None,
        ),
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
            tool_registry: registry,
            tools: tools_arc,
            allowed_tool_names,
            prompt_resolver,
            context_provider: persistent.clone(),
            run_initializer: persistent,
        },
        infra: RuntimeInfra {
            store,
            trace_factory: Arc::new(NoopTraceFactory),
            tool_writer: deps.tool_writer.clone(),
            trace_writer: deps.trace_writer.clone(),
            persist_writer: deps.persist_writer.clone(),
        },
        agent: AgentContext {
            org: Arc::new(LocalOrgServices),
            config: Arc::new(deps.config.clone()),
            cluster_client: None,
            directive: None,
            prompt_config: None,
            prompt_variables: vec![],
            skill_executor: Arc::new(crate::kernel::skills::noop::NoopSkillExecutor),
            memory_recaller: None,
        },
    })
}
