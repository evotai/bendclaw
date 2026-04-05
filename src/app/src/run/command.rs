use std::sync::Arc;
use std::time::Instant;

use bend_base::logx;

use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::run::request::RunRequest;
use crate::run::runner::AgentRunOptions;
use crate::run::runner::AgentRunner;
use crate::run::runner::BendAgentRunner;
use crate::run::sink::EventSink;
use crate::run::stream;
use crate::session;
use crate::storage::model::RunMeta;
use crate::storage::model::RunStatus;
use crate::storage::Storage;

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub session_id: String,
    pub run_id: String,
}

pub async fn run(
    request: RunRequest,
    llm_config: LlmConfig,
    sink: &dyn EventSink,
    storage: &dyn Storage,
) -> Result<RunOutput> {
    let runner = BendAgentRunner::new();
    run_with_runner(request, llm_config, sink, storage, &runner).await
}

pub async fn run_with_runner(
    request: RunRequest,
    llm_config: LlmConfig,
    sink: &dyn EventSink,
    storage: &dyn Storage,
    runner: &dyn AgentRunner,
) -> Result<RunOutput> {
    let started_at = Instant::now();
    let cwd = std::env::current_dir()
        .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))?
        .to_string_lossy()
        .to_string();

    let model = llm_config.model.clone();

    let mut state = if let Some(ref sid) = request.session_id {
        match session::load_session(sid, storage).await? {
            Some(mut s) => {
                s.meta.model = model.clone();
                s
            }
            None => {
                return Err(BendclawError::Session(format!("session not found: {sid}")));
            }
        }
    } else {
        let session_id = crate::ids::new_id();
        session::new_session(session_id, cwd.clone(), model.clone(), storage).await?
    };

    let run_id = crate::ids::new_id();
    let mut run_meta = RunMeta::new(run_id.clone(), state.meta.session_id.clone(), model.clone());
    storage.put_run(run_meta.clone()).await?;
    logx!(
        info,
        "run",
        "started",
        run_id = %run_id,
        session_id = %state.meta.session_id,
        provider = ?llm_config.provider,
        model = %model,
        resumed = request.session_id.is_some(),
    );

    let started_event = stream::run_started_event(&run_id, &state.meta.session_id);
    let mut run_events = vec![started_event.clone()];
    if let Err(e) = sink.publish(Arc::new(started_event.clone())).await {
        run_meta.finish(RunStatus::Failed);
        let _ = storage.put_run(run_meta).await;
        logx!(
            error,
            "run",
            "failed",
            run_id = %run_id,
            session_id = %state.meta.session_id,
            error = %e,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
        );
        return Err(e);
    }

    let query_result = runner
        .run_query(AgentRunOptions {
            llm: llm_config.clone(),
            cwd,
            session_id: state.meta.session_id.clone(),
            messages: state.messages.clone(),
            prompt: request.prompt.clone(),
            max_turns: request.max_turns,
            append_system_prompt: request.append_system_prompt.clone(),
        })
        .await;

    let mut rx = match query_result {
        Ok(rx) => rx,
        Err(e) => {
            run_meta.finish(RunStatus::Failed);
            let _ = storage.put_run(run_meta).await;
            logx!(
                error,
                "run",
                "failed",
                run_id = %run_id,
                session_id = %state.meta.session_id,
                error = %e,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
            );
            return Err(e);
        }
    };

    let mut turn: u32 = 0;
    let mut got_result = false;
    let mut stream_error: Option<BendclawError> = None;

    while let Some(msg) = rx.recv().await {
        if let bend_agent::SDKMessage::Assistant { .. } = &msg {
            turn += 1;
        }

        if let bend_agent::SDKMessage::Result { .. } = &msg {
            got_result = true;
        }

        let event = stream::map_sdk_message(&msg, &run_id, &state.meta.session_id, turn);
        if let Err(e) = sink.publish(Arc::new(event.clone())).await {
            stream_error = Some(e);
            break;
        }
        run_events.push(event);
    }

    let final_messages = runner.take_messages().await;

    if !final_messages.is_empty() {
        session::update_transcript(&mut state, final_messages);
    }

    let save_result = session::save_transcript(&state, storage).await;

    if got_result && save_result.is_ok() && stream_error.is_none() {
        run_meta.finish(RunStatus::Completed);
    } else {
        run_meta.finish(RunStatus::Failed);
    }

    let _ = storage.put_run(run_meta).await;
    let _ = storage.put_run_events(run_events).await;
    runner.close().await;

    if let Some(e) = stream_error {
        logx!(
            error,
            "run",
            "failed",
            run_id = %run_id,
            session_id = %state.meta.session_id,
            error = %e,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            turn,
        );
        return Err(e);
    }

    match &save_result {
        Ok(()) => {
            logx!(
                info,
                "run",
                "completed",
                run_id = %run_id,
                session_id = %state.meta.session_id,
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
                session_id = %state.meta.session_id,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                turn,
            );
        }
    }

    save_result?;

    Ok(RunOutput {
        session_id: state.meta.session_id.clone(),
        run_id,
    })
}
