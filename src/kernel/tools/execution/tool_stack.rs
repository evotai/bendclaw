//! Factory that assembles the full tool execution stack.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel::run::event::Event;
use crate::kernel::skills::runtime::SkillExecutor;
use crate::kernel::tools::definition::toolset::Toolset;
use crate::kernel::tools::execution::tool_events::EventEmitter;
use crate::kernel::tools::execution::tool_executor::CallExecutor;
use crate::kernel::tools::execution::tool_orchestrator::ToolOrchestrator;
use crate::kernel::tools::execution::tool_recorder::ExecutionRecorder;
use crate::kernel::tools::run_labels::RunLabels;
use crate::kernel::tools::ToolContext;
use crate::kernel::trace::Trace;

pub struct ToolStackConfig {
    pub toolset: Toolset,
    pub skill_executor: Arc<dyn SkillExecutor>,
    pub tool_context: ToolContext,
    pub labels: Arc<RunLabels>,
    pub cancel: CancellationToken,
    pub trace: Trace,
    pub event_tx: mpsc::Sender<Event>,
}

pub struct ToolStack {
    pub orchestrator: ToolOrchestrator,
}

impl ToolStack {
    pub fn build(config: ToolStackConfig) -> Self {
        let executor = CallExecutor::new(
            &config.toolset,
            config.skill_executor,
            config.tool_context,
            config.cancel,
            config.event_tx.clone(),
        );
        let recorder = ExecutionRecorder::new(config.labels, config.trace, config.event_tx.clone());
        let emitter = EventEmitter::new(config.event_tx);
        Self {
            orchestrator: ToolOrchestrator::new(executor, recorder, emitter),
        }
    }
}
