use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bend_base::logx;

use crate::agent::AppAgent;
use crate::agent::ExecutionLimits;
use crate::cli::args::CliArgs;
use crate::cli::args::CliCommand;
use crate::cli::create_sink;
use crate::cli::repl::Repl;
use crate::conf::load_config;
use crate::conf::ConfigOverrides;
use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::RunEvent;
use crate::protocol::RunEventContext;
use crate::protocol::RunMeta;
use crate::protocol::RunStatus;
use crate::protocol::TranscriptItem;
use crate::server;
use crate::session::Session;
use crate::storage::open_storage;
use crate::storage::Storage;

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct PromptResult {
    pub session_id: String,
    pub run_id: String,
}

pub struct Cli {
    args: CliArgs,
}

impl Cli {
    pub fn new(args: CliArgs) -> Arc<Self> {
        Arc::new(Self { args })
    }

    pub async fn run(&self) -> Result<()> {
        match (&self.args.prompt, &self.args.command) {
            (Some(_), Some(_)) => Err(BendclawError::Cli(
                "prompt mode and subcommand cannot be used together".into(),
            )),
            (None, None) => self.run_repl().await,
            (Some(prompt), None) => self.run_prompt(prompt.clone()).await,
            (None, Some(CliCommand::Repl)) => self.run_repl().await,
            (None, Some(CliCommand::Server(server_args))) => {
                self.run_server(server_args.port).await
            }
        }
    }

    fn build_limits(&self) -> ExecutionLimits {
        ExecutionLimits {
            max_turns: self.args.max_turns,
            max_total_tokens: self.args.max_tokens,
            max_duration_secs: self.args.max_duration,
        }
    }

    async fn run_prompt(&self, prompt: String) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), None))?;
        let storage = open_storage(&config.storage)?;
        let sink = create_sink(&self.args.output_format);
        let cwd = current_dir()?;

        let mut agent = AppAgent::new(config.active_llm(), &cwd).with_limits(self.build_limits());
        if let Some(sp) = &self.args.append_system_prompt {
            let mut sys = agent.system_prompt().to_string();
            sys.push('\n');
            sys.push_str(sp);
            agent = agent.with_system_prompt(sys);
        }
        let agent = Arc::new(agent);

        let model = agent.llm().model.clone();
        let session = open_session(self.args.resume.as_deref(), &storage, &cwd, &model).await?;
        run_prompt(agent, prompt, session, sink, storage).await?;
        Ok(())
    }

    async fn run_server(&self, port: Option<u16>) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), port))?;
        server::start(config).await
    }

    async fn run_repl(&self) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), None))?;
        let storage = open_storage(&config.storage)?;
        Repl::new(
            config,
            storage,
            self.build_limits(),
            self.args.append_system_prompt.clone(),
            self.args.resume.clone(),
        )?
        .run()
        .await
    }
}

pub async fn run_cli(args: CliArgs) -> Result<()> {
    Cli::new(args).run().await
}

pub async fn run_prompt(
    agent: Arc<AppAgent>,
    prompt: String,
    session: Arc<Session>,
    sink: Arc<dyn EventSink>,
    storage: Arc<dyn Storage>,
) -> Result<PromptResult> {
    let started_at = Instant::now();
    let model = agent.llm().model.clone();

    let session_meta = session.meta().await;

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
        provider = ?agent.llm().provider,
        model = %model,
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

    // Load prior transcripts and prepare incremental transcript building
    let prior_transcripts = session.transcript().await;
    let mut run_transcripts: Vec<TranscriptItem> = vec![TranscriptItem::User {
        text: prompt.clone(),
    }];

    let mut rx = match agent.start(prompt.clone(), &prior_transcripts).await {
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

    while let Some(protocol_event) = rx.recv().await {
        if matches!(protocol_event, crate::protocol::ProtocolEvent::TurnStart) {
            turn += 1;
        }

        // Incrementally build transcript from events
        match &protocol_event {
            crate::protocol::ProtocolEvent::AssistantCompleted {
                content,
                stop_reason,
                ..
            } => {
                let item = crate::protocol::engine::transcript::transcript_from_assistant_completed(
                    content,
                    stop_reason,
                );
                run_transcripts.push(item);
                got_assistant_response = true;
            }
            crate::protocol::ProtocolEvent::ToolEnd {
                tool_call_id,
                tool_name,
                content,
                is_error,
            } => {
                run_transcripts.push(TranscriptItem::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                });
            }
            crate::protocol::ProtocolEvent::TurnEnd => {
                // Persist at each turn boundary so ESC never loses completed turns
                let full: Vec<TranscriptItem> = prior_transcripts
                    .iter()
                    .chain(run_transcripts.iter())
                    .cloned()
                    .collect();
                if let Err(e) = session.apply_and_save(full).await {
                    logx!(
                        error,
                        "run",
                        "incremental_save_failed",
                        run_id = %run_id,
                        session_id = %session_meta.session_id,
                        error = %e,
                    );
                }
            }
            _ => {}
        }

        if let crate::protocol::ProtocolEvent::AgentEnd {
            ref transcripts,
            ref usage,
            transcript_count,
        } = protocol_event
        {
            got_agent_end = true;

            // AgentEnd carries authoritative transcripts — use them for final save
            if !transcripts.is_empty() {
                if let Err(e) = session.apply_and_save(transcripts.clone()).await {
                    logx!(
                        error,
                        "run",
                        "transcript_save_failed",
                        run_id = %run_id,
                        session_id = %session_meta.session_id,
                        error = %e,
                    );
                }
            }

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

    // Fallback save: if abort happened before AgentEnd, save what we have
    if !got_agent_end && run_transcripts.len() > 1 {
        let full: Vec<TranscriptItem> = prior_transcripts
            .iter()
            .chain(run_transcripts.iter())
            .cloned()
            .collect();
        let _ = session.apply_and_save(full).await;
    }

    // Final save: pick up any transcripts the real-time path may have missed
    // (e.g. if the loop exited due to stream_error before AgentEnd).
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

    Ok(PromptResult {
        session_id: session_meta.session_id,
        run_id,
    })
}

fn current_dir() -> Result<String> {
    std::env::current_dir()
        .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))
        .map(|p| p.to_string_lossy().to_string())
}

pub async fn open_session(
    session_id: Option<&str>,
    storage: &Arc<dyn Storage>,
    cwd: &str,
    model: &str,
) -> Result<Arc<Session>> {
    match session_id {
        Some(id) => match Session::load(id, storage.clone()).await? {
            Some(session) => {
                session.set_model(model.to_string()).await;
                Ok(session)
            }
            None => Err(BendclawError::Session(format!("session not found: {id}"))),
        },
        None => {
            let id = crate::ids::new_id();
            Session::create(id, cwd.to_string(), model.to_string(), storage.clone()).await
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
) -> Result<PromptResult> {
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
