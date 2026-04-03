use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::run_deps::RunDeps;
use super::tool_view::ProgressiveToolView;
use crate::execution::compaction::Compactor;
use crate::execution::context::Context;
use crate::execution::event::Event;
use crate::execution::llm::Engine;
use crate::execution::tools::ToolStack;
use crate::execution::tools::ToolStackConfig;
use crate::kernel::trace::TraceRecorder;
use crate::llm::provider::LLMProvider;
use crate::sessions::Message;
use crate::tools::run_labels::RunLabels;
use crate::tools::ToolContext;
use crate::tools::ToolRuntime;

/// High-level run request from session layer.
pub struct RunRequest {
    pub user_id: Arc<str>,
    pub agent_id: Arc<str>,
    pub session_id: Arc<str>,
    pub run_id: String,
    pub turn: u32,
    pub messages: Vec<Message>,
    pub system_prompt: Arc<str>,
    pub is_dispatched: bool,
}

/// Per-run overrides.
pub struct RunConfig {
    pub max_iterations: u32,
    pub max_context_tokens: usize,
    pub max_duration: Duration,
    pub llm: Arc<dyn LLMProvider>,
}

/// Fully assembled run driver — ready to spawn.
pub struct RunDriver {
    pub engine: Engine,
    pub events: mpsc::Receiver<Event>,
    pub cancel: CancellationToken,
    pub iteration: Arc<AtomicU32>,
    pub inbox_tx: mpsc::Sender<Message>,
}

pub fn build_run_driver(
    deps: RunDeps,
    trace: TraceRecorder,
    request: RunRequest,
    config: RunConfig,
) -> RunDriver {
    let tool_view = ProgressiveToolView::new(deps.toolset.tools.clone());
    let ctx = Context {
        agent_id: request.agent_id.clone(),
        user_id: request.user_id.clone(),
        session_id: request.session_id.clone(),
        run_id: request.run_id.as_str().into(),
        turn: request.turn,
        trace_id: trace.trace_id.as_str().into(),
        llm: config.llm.clone(),
        model: config.llm.default_model().into(),
        temperature: config.llm.default_temperature(),
        max_iterations: config.max_iterations,
        max_context_tokens: config.max_context_tokens,
        max_duration: config.max_duration,
        tool_view,
        system_prompt: request.system_prompt,
        messages: request.messages,
    };

    let cancel = CancellationToken::new();
    let iteration = Arc::new(AtomicU32::new(0));
    let compactor = Compactor::new(ctx.llm.clone(), ctx.model.clone(), cancel.clone());

    let (tx, rx) = Engine::create_channel();
    let labels = Arc::new(RunLabels {
        trace_id: trace.trace_id.to_string(),
        run_id: request.run_id.clone(),
        session_id: request.session_id.to_string(),
        agent_id: request.agent_id.to_string(),
    });
    let tool_stack = ToolStack::build(ToolStackConfig {
        toolset: deps.toolset.clone(),
        skill_executor: deps.skill_executor,
        tool_context: ToolContext {
            user_id: request.user_id,
            session_id: request.session_id,
            agent_id: request.agent_id,
            run_id: request.run_id.as_str().into(),
            trace_id: trace.trace_id.as_str().into(),
            workspace: deps.workspace,
            is_dispatched: request.is_dispatched,
            runtime: ToolRuntime {
                event_tx: None,
                cancel: cancel.clone(),
                tool_call_id: None,
            },
            tool_writer: deps.tool_writer,
        },
        labels,
        cancel: cancel.clone(),
        trace: crate::kernel::trace::Trace::new(trace.clone()),
        event_tx: tx.clone(),
    });

    let (inbox_tx, inbox_rx) = Engine::create_inbox();
    let extract_memory = deps.extract_memory;

    let mut engine = Engine::from_tx(
        ctx,
        tool_stack.orchestrator,
        compactor,
        cancel.clone(),
        iteration.clone(),
        trace,
        tx,
        inbox_rx,
        extract_memory,
    );
    if let Some(ref hook) = deps.before_turn_hook {
        engine = engine.with_before_turn(Box::new(Arc::clone(hook)));
    }
    if let Some(ref source) = deps.steering_source {
        engine = engine.with_steering(Box::new(Arc::clone(source)));
    }

    RunDriver {
        engine,
        events: rx,
        cancel,
        iteration,
        inbox_tx,
    }
}
