use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context as TaskContext;
use std::task::Poll;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::kernel::run::persister::TurnPersister;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::session::session::SessionState;
use crate::kernel::ErrorSource;
use crate::kernel::Message;

#[derive(Debug, Clone)]
pub struct FinishedRunOutput {
    pub text: String,
    pub stop_reason: Reason,
}

pub struct Stream {
    task: JoinHandle<Result<AgentResult>>,
    events: mpsc::Receiver<Event>,
    injected: mpsc::Receiver<Event>,
    state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    persister: TurnPersister,
    usage_provider: String,
    usage_model: String,
    collected_events: Vec<Event>,
    yield_first: VecDeque<Event>,
}

impl Stream {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        task: JoinHandle<Result<AgentResult>>,
        events: mpsc::Receiver<Event>,
        injected: mpsc::Receiver<Event>,
        state: Arc<Mutex<SessionState>>,
        history: Arc<Mutex<Vec<Message>>>,
        persister: TurnPersister,
        usage_provider: String,
        usage_model: String,
        initial_events: Vec<Event>,
    ) -> Self {
        Self {
            task,
            events,
            injected,
            state,
            history,
            persister,
            usage_provider,
            usage_model,
            collected_events: initial_events,
            yield_first: VecDeque::new(),
        }
    }

    pub fn run_id(&self) -> &str {
        self.persister.run_id()
    }

    /// Prepend an event to be yielded before any engine events.
    /// The event is also added to collected_events for persistence.
    pub(crate) fn prepend_event(&mut self, event: Event) {
        self.yield_first.push_back(event);
    }

    pub async fn finish(self) -> Result<String> {
        Ok(self.finish_output().await?.text)
    }

    pub async fn finish_output(mut self) -> Result<FinishedRunOutput> {
        while let Some(event) = self.events.recv().await {
            self.collect_runtime_info(&event);
            self.collected_events.push(event);
        }
        // Drain any injected events that arrived before the stream closed.
        while let Ok(event) = self.injected.try_recv() {
            self.collected_events.push(event);
        }
        *self.state.lock() = SessionState::Idle;
        let task_result = (&mut self.task).await;
        match task_result {
            Ok(Ok(result)) => {
                let stop_reason = result.stop_reason.clone();
                *self.history.lock() = result.messages.clone();
                let text = self.persister.persist_success(
                    result,
                    &self.usage_provider,
                    &self.usage_model,
                    &self.collected_events,
                )?;
                Ok(FinishedRunOutput { text, stop_reason })
            }
            Ok(Err(e)) => {
                let text = Message::error(ErrorSource::Internal, format!("{e}")).text();
                self.persister.persist_error(&e, &self.collected_events);
                Ok(FinishedRunOutput {
                    text,
                    stop_reason: Reason::Error,
                })
            }
            Err(e) if e.is_cancelled() => {
                let text = AgentResult::aborted().text();
                self.persister.persist_cancelled(&self.collected_events);
                Ok(FinishedRunOutput {
                    text,
                    stop_reason: Reason::Aborted,
                })
            }
            Err(e) => {
                let err = ErrorCode::internal(format!("agent task failed: {e}"));
                let text = Message::error(ErrorSource::Internal, format!("{err}")).text();
                self.persister.persist_error(&err, &self.collected_events);
                Ok(FinishedRunOutput {
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
        if let Some(event) = self.yield_first.pop_front() {
            self.collected_events.push(event.clone());
            return Poll::Ready(Some(event));
        }
        // Drain any externally injected events (e.g. DecisionRequired) before engine events.
        if let Poll::Ready(Some(event)) = self.injected.poll_recv(cx) {
            self.collected_events.push(event.clone());
            return Poll::Ready(Some(event));
        }
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
