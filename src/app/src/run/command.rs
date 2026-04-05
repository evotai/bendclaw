use std::sync::Arc;

use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::run::model::RunMeta;
use crate::run::model::RunStatus;
use crate::run::request::RunRequest;
use crate::run::runner::AgentRunOptions;
use crate::run::runner::AgentRunner;
use crate::run::runner::BendAgentRunner;
use crate::run::sink::EventSink;
use crate::run::stream;
use crate::session;
use crate::store::RunStore;
use crate::store::SessionStore;

pub async fn run(
    request: RunRequest,
    llm_config: LlmConfig,
    sink: &dyn EventSink,
    session_store: &dyn SessionStore,
    run_store: &dyn RunStore,
) -> Result<()> {
    let runner = BendAgentRunner::new();
    run_with_runner(request, llm_config, sink, session_store, run_store, &runner).await
}

pub async fn run_with_runner(
    request: RunRequest,
    llm_config: LlmConfig,
    sink: &dyn EventSink,
    session_store: &dyn SessionStore,
    run_store: &dyn RunStore,
    runner: &dyn AgentRunner,
) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))?
        .to_string_lossy()
        .to_string();

    let model = llm_config.model.clone();

    let mut state = if let Some(ref sid) = request.session_id {
        match session::load_session(sid, session_store).await? {
            Some(mut s) => {
                s.meta.model = model.clone();
                s
            }
            None => {
                return Err(BendclawError::Session(format!("session not found: {sid}")));
            }
        }
    } else {
        let session_id = ulid::Ulid::new().to_string();
        session::new_session(session_id, cwd.clone(), model.clone(), session_store).await?
    };

    let run_id = ulid::Ulid::new().to_string();
    let mut run_meta = RunMeta::new(run_id.clone(), state.meta.session_id.clone(), model.clone());
    run_store.save_run(&run_meta).await?;

    let started_event = stream::run_started_event(&run_id, &state.meta.session_id);
    if let Err(e) = sink.publish(Arc::new(started_event.clone())).await {
        run_meta.finish(RunStatus::Failed);
        let _ = run_store.save_run(&run_meta).await;
        return Err(e);
    }
    if let Err(e) = run_store.append_event(&run_id, &started_event).await {
        run_meta.finish(RunStatus::Failed);
        let _ = run_store.save_run(&run_meta).await;
        return Err(e);
    }

    let query_result = runner
        .run_query(AgentRunOptions {
            llm: llm_config.clone(),
            cwd,
            messages: state.messages.clone(),
            prompt: request.prompt.clone(),
        })
        .await;

    let mut rx = match query_result {
        Ok(rx) => rx,
        Err(e) => {
            run_meta.finish(RunStatus::Failed);
            let _ = run_store.save_run(&run_meta).await;
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
        if let Err(e) = run_store.append_event(&run_id, &event).await {
            stream_error = Some(e);
            break;
        }
    }

    let final_messages = runner.take_messages().await;

    if !final_messages.is_empty() {
        session::update_transcript(&mut state, final_messages);
    }

    let save_result = session::save_transcript(&state, session_store).await;

    if got_result && save_result.is_ok() && stream_error.is_none() {
        run_meta.finish(RunStatus::Completed);
    } else {
        run_meta.finish(RunStatus::Failed);
    }

    let _ = run_store.save_run(&run_meta).await;
    runner.close().await;

    if let Some(e) = stream_error {
        return Err(e);
    }

    save_result
}
