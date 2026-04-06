use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bend_base::logx;

use super::RequestAgent;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::ProtocolEvent;
use crate::protocol::RunEvent;
use crate::protocol::RunEventContext;
use crate::protocol::RunMeta;
use crate::protocol::RunStatus;
use crate::protocol::TranscriptItem;
use crate::session::Session;
use crate::storage::Storage;

// ---------------------------------------------------------------------------
// ExecutionLimits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_duration_secs: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 512,
            max_total_tokens: 100_000_000,
            max_duration_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Request {
    prompt: String,
    session_id: Option<String>,
    limits: ExecutionLimits,
    append_system_prompt: Option<String>,
}

impl Request {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            session_id: None,
            limits: ExecutionLimits::default(),
            append_system_prompt: None,
        }
    }

    pub fn with_session(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn with_limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.append_system_prompt = Some(prompt.into());
        self
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn limits(&self) -> &ExecutionLimits {
        &self.limits
    }

    pub fn append_system_prompt(&self) -> Option<&str> {
        self.append_system_prompt.as_deref()
    }

    /// One-shot execution (creates its own agent).
    pub async fn execute(
        self,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
    ) -> Result<RequestResult> {
        self.execute_with_agent(llm, sink, storage, RequestAgent::new())
            .await
    }

    /// Execution with a shared agent (for repl multi-turn).
    pub async fn execute_with_agent(
        self,
        llm: LlmConfig,
        sink: Arc<dyn EventSink>,
        storage: Arc<dyn Storage>,
        agent: Arc<RequestAgent>,
    ) -> Result<RequestResult> {
        run_request(self, llm, sink, storage, agent).await
    }
}

// PLACEHOLDER_REMAINING

// ---------------------------------------------------------------------------
// RequestOptions (internal, resolved context)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RequestOptions {
    pub llm: LlmConfig,
    pub cwd: String,
    pub transcript: Vec<TranscriptItem>,
    pub prompt: String,
    pub limits: ExecutionLimits,
    pub append_system_prompt: Option<String>,
}

// ---------------------------------------------------------------------------
// RequestResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RequestResult {
    pub session_id: String,
    pub run_id: String,
}

// ---------------------------------------------------------------------------
// EventSink
// ---------------------------------------------------------------------------

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()>;
}

// ---------------------------------------------------------------------------
// run_request (moved from execute.rs)
// ---------------------------------------------------------------------------

async fn run_request(
    request: Request,
    llm: LlmConfig,
    sink: Arc<dyn EventSink>,
    storage: Arc<dyn Storage>,
    agent: Arc<RequestAgent>,
) -> Result<RequestResult> {
    let started_at = Instant::now();
    let cwd = std::env::current_dir()
        .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))?
        .to_string_lossy()
        .to_string();
    let model = llm.model.clone();
    let session = open_session(&request, &storage, &cwd, &model).await?;
    let session_meta = session.meta().await;

    // PLACEHOLDER_RUN_BODY

    let run_id = crate::ids::new_id();
    let mut run_meta = RunMeta::new(
        run_id.clone(),
        session_meta.session_id.clone(),
        model.clone(),
    );
    storage.put_run(run_meta.clone()).await?;

    logx!(
        info,
        "run",
        "started",
        run_id = %run_id,
        session_id = %session_meta.session_id,
        provider = ?llm.provider,
        model = %model,
        resumed = request.session_id.is_some(),
    );

    let started_event = RunEventContext::new(&run_id, &session_meta.session_id, 0).started();
    let mut run_events = vec![started_event.clone()];
    if let Err(error) = sink.publish(Arc::new(started_event)).await {
        return fail_run(
            &storage,
            &mut run_meta,
            &run_id,
            &session_meta.session_id,
            &started_at,
            error,
        )
        .await;
    }

    let mut rx = match agent
        .start(RequestOptions {
            llm: llm.clone(),
            cwd,
            transcript: session.transcript().await,
            prompt: request.prompt.clone(),
            limits: request.limits.clone(),
            append_system_prompt: request.append_system_prompt.clone(),
        })
        .await
    {
        Ok(rx) => rx,
        Err(error) => {
            return fail_run(
                &storage,
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

    // PLACEHOLDER_EVENT_LOOP

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
            if let Err(error) = sink.publish(Arc::new(finished_event.clone())).await {
                stream_error = Some(error);
            }
            run_events.push(finished_event);
            continue;
        }

        let event_context = RunEventContext::new(&run_id, &session_meta.session_id, turn);
        if let Some(event) = event_context.map(&protocol_event) {
            if let Err(error) = sink.publish(Arc::new(event.clone())).await {
                stream_error = Some(error);
                break;
            }
            run_events.push(event);
        }
    }

    // PLACEHOLDER_FINALIZE

    let final_transcripts = agent.take_transcripts().await;
    if !final_transcripts.is_empty() {
        session.apply_transcript(final_transcripts).await;
    }

    let save_result = session.save().await;

    if got_agent_end && got_assistant_response && save_result.is_ok() && stream_error.is_none() {
        run_meta.finish(RunStatus::Completed);
    } else {
        run_meta.finish(RunStatus::Failed);
    }

    let _ = storage.put_run(run_meta).await;
    let _ = storage.put_run_events(run_events).await;
    agent.close().await;

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn open_session(
    request: &Request,
    storage: &Arc<dyn Storage>,
    cwd: &str,
    model: &str,
) -> Result<Arc<Session>> {
    match request.session_id() {
        Some(session_id) => match Session::load(session_id, storage.clone()).await? {
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
                storage.clone(),
            )
            .await
        }
    }
}

async fn fail_run(
    storage: &Arc<dyn Storage>,
    run_meta: &mut RunMeta,
    run_id: &str,
    session_id: &str,
    started_at: &Instant,
    error: BendclawError,
) -> Result<RequestResult> {
    run_meta.finish(RunStatus::Failed);
    let _ = storage.put_run(run_meta.clone()).await;
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
