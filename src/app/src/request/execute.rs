use std::sync::Arc;
use std::time::Instant;

use bend_base::logx;

use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::ProtocolEvent;
use crate::protocol::RunEventContext;
use crate::protocol::RunMeta;
use crate::protocol::RunStatus;
use crate::protocol::TranscriptItem;
use crate::request::EventSink;
use crate::request::Request;
use crate::request::RequestAgent;
use crate::request::RequestOptions;
use crate::request::RequestResult;
use crate::session::Session;
use crate::storage::Storage;

pub struct RequestExecutor {
    request: Request,
    llm: LlmConfig,
    sink: Arc<dyn EventSink>,
    storage: Arc<dyn Storage>,
    agent: Arc<RequestAgent>,
}

impl RequestExecutor {
    pub fn new(
        request: Request,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
        agent: Arc<RequestAgent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            request,
            llm,
            sink,
            storage,
            agent,
        })
    }

    pub fn open(
        request: Request,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
    ) -> Arc<Self> {
        Self::new(request, llm, sink, storage, RequestAgent::new())
    }

    pub async fn execute(&self) -> Result<RequestResult> {
        let started_at = Instant::now();
        let cwd = std::env::current_dir()
            .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))?
            .to_string_lossy()
            .to_string();
        let model = self.llm.model.clone();
        let session = self.open_session(&cwd, &model).await?;
        let session_meta = session.meta().await;

        let run_id = crate::ids::new_id();
        let mut run_meta = RunMeta::new(
            run_id.clone(),
            session_meta.session_id.clone(),
            model.clone(),
        );
        self.storage.put_run(run_meta.clone()).await?;

        logx!(
            info,
            "run",
            "started",
            run_id = %run_id,
            session_id = %session_meta.session_id,
            provider = ?self.llm.provider,
            model = %model,
            resumed = self.request.session_id.is_some(),
        );

        let started_event = RunEventContext::new(&run_id, &session_meta.session_id, 0).started();
        let mut run_events = vec![started_event.clone()];
        if let Err(error) = self.sink.publish(Arc::new(started_event)).await {
            return self
                .fail_run(
                    &mut run_meta,
                    &run_id,
                    &session_meta.session_id,
                    &started_at,
                    error,
                )
                .await;
        }

        let mut rx = match self
            .agent
            .start(RequestOptions {
                llm: self.llm.clone(),
                cwd,
                transcript: session.transcript().await,
                prompt: self.request.prompt.clone(),
                max_turns: self.request.max_turns,
                append_system_prompt: self.request.append_system_prompt.clone(),
            })
            .await
        {
            Ok(rx) => rx,
            Err(error) => {
                return self
                    .fail_run(
                        &mut run_meta,
                        &run_id,
                        &session_meta.session_id,
                        &started_at,
                        error,
                    )
                    .await;
            }
        };

        let mut turn = 0_u32;
        let mut got_agent_end = false;
        let mut got_assistant_response = false;
        let mut stream_error = None;

        while let Some(protocol_event) = rx.recv().await {
            if matches!(protocol_event, ProtocolEvent::TurnStart) {
                turn += 1;
            }

            if let ProtocolEvent::AgentEnd {
                ref transcripts,
                ref usage,
                transcript_count,
            } = protocol_event
            {
                got_agent_end = true;
                got_assistant_response = transcripts
                    .iter()
                    .any(|t| matches!(t, TranscriptItem::Assistant { .. }));

                let last_text = transcripts
                    .iter()
                    .rev()
                    .find_map(|t| {
                        if let TranscriptItem::Assistant { text, .. } = t {
                            if !text.is_empty() {
                                return Some(text.clone());
                            }
                        }
                        None
                    })
                    .unwrap_or_default();

                let finished_event = RunEventContext::new(&run_id, &session_meta.session_id, turn)
                    .finished(
                        last_text,
                        usage.clone(),
                        turn,
                        started_at.elapsed().as_millis() as u64,
                        transcript_count,
                    );
                if let Err(error) = self.sink.publish(Arc::new(finished_event.clone())).await {
                    stream_error = Some(error);
                }
                run_events.push(finished_event);
                continue;
            }

            let event_context = RunEventContext::new(&run_id, &session_meta.session_id, turn);
            if let Some(event) = event_context.map(&protocol_event) {
                if let Err(error) = self.sink.publish(Arc::new(event.clone())).await {
                    stream_error = Some(error);
                    break;
                }
                run_events.push(event);
            }
        }

        let final_transcripts = self.agent.take_transcripts().await;
        if !final_transcripts.is_empty() {
            session.apply_transcript(final_transcripts).await;
        }

        let save_result = session.save().await;

        if got_agent_end && got_assistant_response && save_result.is_ok() && stream_error.is_none()
        {
            run_meta.finish(RunStatus::Completed);
        } else {
            run_meta.finish(RunStatus::Failed);
        }

        let _ = self.storage.put_run(run_meta).await;
        let _ = self.storage.put_run_events(run_events).await;
        self.agent.close().await;

        if let Some(error) = stream_error {
            logx!(
                error,
                "run",
                "failed",
                run_id = %run_id,
                session_id = %session_meta.session_id,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                turn,
            );
            return Err(error);
        }

        match &save_result {
            Ok(()) => {
                logx!(
                    info,
                    "run",
                    "completed",
                    run_id = %run_id,
                    session_id = %session_meta.session_id,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    turn,
                );
            }
            Err(error) => {
                logx!(
                    error,
                    "run",
                    "failed",
                    run_id = %run_id,
                    session_id = %session_meta.session_id,
                    error = %error,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    turn,
                );
            }
        }

        save_result?;

        Ok(RequestResult {
            session_id: session_meta.session_id,
            run_id,
        })
    }

    async fn open_session(&self, cwd: &str, model: &str) -> Result<Arc<Session>> {
        match &self.request.session_id {
            Some(session_id) => match Session::load(session_id, self.storage.clone()).await? {
                Some(session) => {
                    session.set_model(model.to_string()).await;
                    Ok(session)
                }
                None => Err(BendclawError::Session(format!(
                    "session not found: {session_id}"
                ))),
            },
            None => {
                let session_id = crate::ids::new_id();
                Session::create(
                    session_id,
                    cwd.to_string(),
                    model.to_string(),
                    self.storage.clone(),
                )
                .await
            }
        }
    }

    async fn fail_run(
        &self,
        run_meta: &mut RunMeta,
        run_id: &str,
        session_id: &str,
        started_at: &Instant,
        error: BendclawError,
    ) -> Result<RequestResult> {
        run_meta.finish(RunStatus::Failed);
        let _ = self.storage.put_run(run_meta.clone()).await;
        logx!(
            error,
            "run",
            "failed",
            run_id = %run_id,
            session_id = %session_id,
            error = %error,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
        );
        Err(error)
    }
}
