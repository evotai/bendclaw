use std::pin::Pin;
use std::sync::Arc;
use std::task::Context as TaskContext;
use std::task::Poll;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::execution::event::Delta;
use crate::execution::event::Event;
use crate::execution::result::Reason;
use crate::execution::result::Result as AgentResult;
use crate::execution::result::RunOutput;
use crate::sessions::backend::sink::RunPersister;
use crate::sessions::core::session_state::SessionState;
use crate::sessions::Message;
use crate::types::ErrorCode;
use crate::types::ErrorSource;
use crate::types::Result;

/// Backward-compatible alias. New code should use `RunOutput` directly.
pub type FinishedRunOutput = RunOutput;

pub struct Stream {
    task: JoinHandle<Result<AgentResult>>,
    events: mpsc::Receiver<Event>,
    state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    run_sink: Arc<dyn RunPersister>,
    run_id: String,
    usage_provider: String,
    usage_model: String,
    collected_events: Vec<Event>,
}

impl Stream {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        task: JoinHandle<Result<AgentResult>>,
        events: mpsc::Receiver<Event>,
        state: Arc<Mutex<SessionState>>,
        history: Arc<Mutex<Vec<Message>>>,
        run_sink: Arc<dyn RunPersister>,
        run_id: String,
        usage_provider: String,
        usage_model: String,
        initial_events: Vec<Event>,
    ) -> Self {
        Self {
            task,
            events,
            state,
            history,
            run_sink,
            run_id,
            usage_provider,
            usage_model,
            collected_events: initial_events,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub async fn finish(self) -> Result<String> {
        Ok(self.finish_output().await?.text)
    }

    pub async fn finish_output(mut self) -> Result<RunOutput> {
        while let Some(event) = self.events.recv().await {
            self.collect_runtime_info(&event);
            self.collected_events.push(event);
        }
        *self.state.lock() = SessionState::Idle;
        let task_result = (&mut self.task).await;
        match task_result {
            Ok(Ok(result)) => {
                let stop_reason = result.stop_reason.clone();
                *self.history.lock() = result.messages.clone();
                let text = result.text();
                self.run_sink.persist_success(
                    result,
                    &self.usage_provider,
                    &self.usage_model,
                    &self.collected_events,
                );
                Ok(RunOutput { text, stop_reason })
            }
            Ok(Err(e)) => {
                let text = Message::error(ErrorSource::Internal, format!("{e}")).text();
                self.run_sink.persist_error(&e, &self.collected_events);
                Ok(RunOutput {
                    text,
                    stop_reason: Reason::Error,
                })
            }
            Err(e) if e.is_cancelled() => {
                let text = AgentResult::aborted().text();
                self.run_sink.persist_cancelled(&self.collected_events);
                Ok(RunOutput {
                    text,
                    stop_reason: Reason::Aborted,
                })
            }
            Err(e) => {
                let err = ErrorCode::internal(format!("agent task failed: {e}"));
                let text = Message::error(ErrorSource::Internal, format!("{err}")).text();
                self.run_sink.persist_error(&err, &self.collected_events);
                Ok(RunOutput {
                    text,
                    stop_reason: Reason::Error,
                })
            }
        }
    }

    fn collect_runtime_info(&mut self, event: &Event) {
        if let Event::StreamDelta(Delta::Done {
            provider, model, ..
        }) = event
        {
            if let Some(p) = provider {
                self.usage_provider = p.clone();
            }
            if let Some(m) = model {
                self.usage_model = m.clone();
            }
        }
    }
}

impl tokio_stream::Stream for Stream {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        match self.events.poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                self.collect_runtime_info(&event);
                self.collected_events.push(event.clone());
                Poll::Ready(Some(event))
            }
            other => other,
        }
    }
}
