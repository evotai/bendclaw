use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::kernel::run::event::Event;
use crate::kernel::run::planning::build_run_driver;
use crate::kernel::run::planning::RunConfig;
use crate::kernel::run::planning::RunDeps;
use crate::kernel::run::planning::RunRequest;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::Message;
use crate::types::Result as AgentBaseResult;

pub struct EngineHandle {
    pub task: JoinHandle<AgentBaseResult<AgentResult>>,
    pub events: mpsc::Receiver<Event>,
    pub cancel: CancellationToken,
    pub iteration: Arc<AtomicU32>,
    pub inbox_tx: mpsc::Sender<Message>,
}

pub fn launch(
    deps: RunDeps,
    trace: TraceRecorder,
    request: RunRequest,
    config: RunConfig,
) -> EngineHandle {
    let mut driver = build_run_driver(deps, trace, request, config);
    let task = tokio::spawn(async move { driver.engine.run().await });
    EngineHandle {
        task,
        events: driver.events,
        cancel: driver.cancel,
        iteration: driver.iteration,
        inbox_tx: driver.inbox_tx,
    }
}
