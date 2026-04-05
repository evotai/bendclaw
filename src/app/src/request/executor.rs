use std::sync::Arc;
use std::time::Instant;

use bend_base::logx;

use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::request::map_sdk_message;
use crate::request::request_started_event;
use crate::request::EventSink;
use crate::request::Request;
use crate::request::RequestOptions;
use crate::request::RequestResult;
use crate::request::RequestRunner;
use crate::session::Session;
use crate::storage::model::RunMeta;
use crate::storage::model::RunStatus;
use crate::storage::Storage;

pub struct RequestExecutor {
    request: Request,
    llm: LlmConfig,
    sink: Arc<dyn EventSink>,
    storage: Arc<dyn Storage>,
    runner: Arc<RequestRunner>,
}

impl RequestExecutor {
    pub fn new(
        request: Request,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
        runner: Arc<RequestRunner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            request,
            llm,
            sink,
            storage,
            runner,
        })
    }

    pub fn open(
        request: Request,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
    ) -> Arc<Self> {
        Self::new(request, llm, sink, storage, RequestRunner::new())
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

        let started_event = request_started_event(&run_id, &session_meta.session_id);
        let mut run_events = vec![started_event.clone()];
        if let Err(error) = self.sink.publish(Arc::new(started_event)).await {
            run_meta.finish(RunStatus::Failed);
            let _ = self.storage.put_run(run_meta).await;
            logx!(
                error,
                "run",
                "failed",
                run_id = %run_id,
                session_id = %session_meta.session_id,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
            );
            return Err(error);
        }

        let mut rx = match self
            .runner
            .run_query(RequestOptions {
                llm: self.llm.clone(),
                cwd,
                session_id: session_meta.session_id.clone(),
                messages: session.messages().await,
                prompt: self.request.prompt.clone(),
                max_turns: self.request.max_turns,
                append_system_prompt: self.request.append_system_prompt.clone(),
            })
            .await
        {
            Ok(rx) => rx,
            Err(error) => {
                run_meta.finish(RunStatus::Failed);
                let _ = self.storage.put_run(run_meta).await;
                logx!(
                    error,
                    "run",
                    "failed",
                    run_id = %run_id,
                    session_id = %session_meta.session_id,
                    error = %error,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                );
                return Err(error);
            }
        };

        let mut turn = 0_u32;
        let mut got_result = false;
        let mut stream_error = None;

        while let Some(message) = rx.recv().await {
            if matches!(message, bend_agent::SDKMessage::Assistant { .. }) {
                turn += 1;
            }

            if matches!(message, bend_agent::SDKMessage::Result { .. }) {
                got_result = true;
            }

            let event = map_sdk_message(&message, &run_id, &session_meta.session_id, turn);
            if let Err(error) = self.sink.publish(Arc::new(event.clone())).await {
                stream_error = Some(error);
                break;
            }
            run_events.push(event);
        }

        let final_messages = self.runner.take_messages().await;
        if !final_messages.is_empty() {
            session.apply_messages(final_messages).await;
        }

        let save_result = session.save().await;

        if got_result && save_result.is_ok() && stream_error.is_none() {
            run_meta.finish(RunStatus::Completed);
        } else {
            run_meta.finish(RunStatus::Failed);
        }

        let _ = self.storage.put_run(run_meta).await;
        let _ = self.storage.put_run_events(run_events).await;
        self.runner.close().await;

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
}
