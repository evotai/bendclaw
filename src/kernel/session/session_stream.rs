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
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::session::session::SessionState;
use crate::kernel::ErrorSource;
use crate::kernel::Message;

pub struct Stream {
    task: JoinHandle<Result<AgentResult>>,
    events: mpsc::Receiver<Event>,
    state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    persister: TurnPersister,
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
        persister: TurnPersister,
        usage_provider: String,
        usage_model: String,
        initial_events: Vec<Event>,
    ) -> Self {
        Self {
            task,
            events,
            state,
            history,
            persister,
            usage_provider,
            usage_model,
            collected_events: initial_events,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.persister.run_id
    }

    pub async fn finish(mut self) -> Result<String> {
        while let Some(event) = self.events.recv().await {
            self.collect_runtime_info(&event);
            self.collected_events.push(event);
        }
        *self.state.lock() = SessionState::Idle;
        let task_result = (&mut self.task).await;
        match task_result {
            Ok(Ok(result)) => {
                *self.history.lock() = result.messages.clone();
                self.persister
                    .persist_success(
                        result,
                        &self.usage_provider,
                        &self.usage_model,
                        &self.collected_events,
                    )
                    .await
            }
            Ok(Err(e)) => {
                self.persister
                    .persist_error(&e, &self.collected_events)
                    .await;
                Ok(Message::error(ErrorSource::Internal, format!("{e}")).text())
            }
            Err(e) if e.is_cancelled() => {
                self.persister
                    .persist_cancelled(&self.collected_events)
                    .await;
                Ok(AgentResult::aborted().text())
            }
            Err(e) => {
                let err = ErrorCode::internal(format!("agent task failed: {e}"));
                self.persister
                    .persist_error(&err, &self.collected_events)
                    .await;
                Ok(Message::error(ErrorSource::Internal, format!("{err}")).text())
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
