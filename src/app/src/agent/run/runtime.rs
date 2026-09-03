//! Engine runtime — create engine, forward events, orchestrate a run.
//!
//! This module owns the boundary between `evot_engine::AgentEvent` and the
//! app-layer `RunEvent`. No engine types leak beyond this module.
//!
//! A single `Run` comprises one engine turn. Consumers see one `RunStarted`
//! and one aggregated `RunFinished`.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::control::RunControl;
use super::convert::assistant_blocks_from_content;
use super::convert::extract_content_text;
use super::convert::from_agent_messages;
use super::convert::total_usage;
use super::convert::transcript_from_agent_message;
use super::event::AssistantContentType;
use super::event::LlmMessageStats;
use super::event::LlmToolCallSummary;
use super::event::RunEvent;
use super::event::RunEventContext;
use super::event::RunEventPayload;
use super::event::ToolCallStreamPhase;
use super::run::Run;
use crate::agent::session::Session;
use crate::conf::Protocol;
use crate::error::Result;
use crate::types::CompactRecord;
use crate::types::ContextCompactionCompletedStats;
use crate::types::ContextCompactionStartedStats;
use crate::types::LlmCallCompletedStats;
use crate::types::LlmCallMetrics;
use crate::types::LlmCallRetryStats;
use crate::types::LlmCallStartedStats;
use crate::types::RunFinishedStats;
use crate::types::ToolDef;
use crate::types::ToolFinishedStats;
use crate::types::TranscriptItem;
use crate::types::TranscriptStats;
use crate::types::UsageSummary;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

pub struct EngineOptions {
    pub provider: String,
    pub protocol: Protocol,
    pub model: String,
    pub api_key: String,
    pub model_config: evot_engine::provider::ModelConfig,
    pub system_prompt: String,
    /// Per-section breakdown matching `system_prompt`. Guaranteed:
    /// `system_prompt == sections.iter().map(|s| s.text).join("\n\n")`.
    pub system_prompt_sections: Vec<crate::agent::prompt::Section>,
    /// Execution limits, or `None` for interactive runs (no limits — the loop
    /// stops only on error, abort, or when there is no more work, matching pi).
    pub limits: Option<crate::agent::ExecutionLimits>,
    pub tools: Vec<Box<dyn evot_engine::AgentTool>>,
    pub thinking_level: evot_engine::ThinkingLevel,
    pub cwd: std::path::PathBuf,
    pub path_guard: std::sync::Arc<evot_engine::PathGuard>,
    pub spill_dir: Option<std::path::PathBuf>,
    pub process_manager: Option<Arc<evot_engine::tools::ProcessManager>>,
    pub prompt_cache_key: Option<String>,
    pub provider_override: Option<Arc<dyn evot_engine::provider::StreamProvider>>,
    /// Cross-compaction state restored from the session's latest `Compact`
    /// item, so in-run auto-compaction updates the previous summary.
    pub compaction_state: Option<evot_engine::CompactionState>,
}

// ---------------------------------------------------------------------------
// TurnInput — prepared by agent, executed by runtime
// ---------------------------------------------------------------------------

pub(in crate::agent) struct TurnInput {
    pub options: EngineOptions,
    pub history: Vec<evot_engine::AgentMessage>,
    pub input: Vec<evot_engine::Content>,
    pub session: Arc<Session>,
    /// Persisted transcript generation used to build `history`.
    pub transcript_seq: u64,
}

// ---------------------------------------------------------------------------
// TurnFactory — caller-provided builder of per-turn engine state
// ---------------------------------------------------------------------------

/// Rebuilds the engine input for the next internal turn.
///
/// The runtime calls this once per turn. The factory captures whatever
/// the agent layer needs (config, sandbox, tools, system prompt) and
/// resolves the latest history at call time.
#[async_trait]
pub(in crate::agent) trait TurnFactory: Send + Sync + 'static {
    /// Build the engine options + history + initial user input for this turn.
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<TurnInput>;
}

// ---------------------------------------------------------------------------
// execute_run — public entry point: schedule a Run asynchronously
// ---------------------------------------------------------------------------

pub(in crate::agent) struct ExecuteRunArgs {
    pub run_id: String,
    pub session_id: String,
    pub session: Arc<Session>,
    pub initial_input: Vec<evot_engine::Content>,
    pub factory: Arc<dyn TurnFactory>,
    pub on_complete: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub(in crate::agent) fn execute_run(args: ExecuteRunArgs) -> Run {
    let (tx, rx) = mpsc::unbounded_channel();
    let control = RunControl::new();
    let run = Run::new(
        args.run_id.clone(),
        args.session_id.clone(),
        rx,
        control.clone(),
    );
    tokio::spawn(run_loop(args, tx, control));
    run
}

// ---------------------------------------------------------------------------
// RuntimeEvent — private orchestration signal
// ---------------------------------------------------------------------------

enum RuntimeEvent {
    Public(RunEventPayload),
    Transcript(TranscriptItem),
    TurnStarted,
    TurnEnded,
    EngineCompleted {
        last_text: String,
        usage: UsageSummary,
        transcript_count: usize,
    },
    CompactionCompleted {
        reason: crate::types::CompactReason,
        result: crate::types::CompactionResult,
        summary: Option<String>,
        messages: Vec<evot_engine::AgentMessage>,
        state: evot_engine::CompactionState,
        context_window: usize,
        will_retry: bool,
    },
}

// ---------------------------------------------------------------------------
// run_loop — outer loop: drive engine turns
// ---------------------------------------------------------------------------

async fn run_loop(args: ExecuteRunArgs, tx: mpsc::UnboundedSender<RunEvent>, control: RunControl) {
    let ExecuteRunArgs {
        run_id,
        session_id,
        session,
        initial_input,
        factory,
        on_complete,
    } = args;

    let started_at = Instant::now();
    let _ = tx.send(RunEventContext::new(&run_id, &session_id, 0).started());

    let mut total_usage = UsageSummary::default();
    let mut total_turns: u32 = 0;
    let mut total_transcripts: usize = 0;
    let mut last_text = String::new();
    let mut compact_records: Vec<CompactRecord> = Vec::new();

    if !control.is_cancelled() {
        let outcome = match factory.build(initial_input).await {
            Ok(turn) => {
                Some(drive_one_turn(turn, &tx, &control, &run_id, &session_id, started_at).await)
            }
            Err(e) => {
                tracing::error!(
                    stage = "run",
                    status = "build_turn_failed",
                    run_id = %run_id,
                    session_id = %session_id,
                    error = %e,
                );
                // Surface the failure to the caller instead of ending the run
                // silently — e.g. a missing API key must be visible in the UI.
                let _ = tx.send(RunEventContext::new(&run_id, &session_id, 0).event(
                    RunEventPayload::Error {
                        message: e.to_string(),
                    },
                ));
                None
            }
        };

        if let Some(outcome) = outcome {
            let TurnOutcome {
                turn_count,
                usage,
                transcript_count,
                last_text: turn_last_text,
                compact_records: turn_compacts,
                engine_completed: _,
                transcript: _,
            } = outcome;

            total_turns = total_turns.saturating_add(turn_count);
            total_usage.input = total_usage.input.saturating_add(usage.input);
            total_usage.output = total_usage.output.saturating_add(usage.output);
            total_usage.cache_read = total_usage.cache_read.saturating_add(usage.cache_read);
            total_usage.cache_write = total_usage.cache_write.saturating_add(usage.cache_write);
            total_transcripts = total_transcripts.saturating_add(transcript_count);
            if !turn_last_text.is_empty() {
                last_text = turn_last_text;
            }
            compact_records.extend(turn_compacts);
        }
    }

    // Emit the aggregated run-finished event and a single transcript stats
    // record so consumers see one Run, not N internal turns.
    let duration_ms = started_at.elapsed().as_millis() as u64;
    let stats = TranscriptStats::RunFinished(RunFinishedStats {
        usage: total_usage.clone(),
        turn_count: total_turns,
        duration_ms,
        transcript_count: total_transcripts,
    });
    if let Err(e) = session.write_items(vec![stats.to_item()]).await {
        tracing::warn!(
            stage = "run",
            status = "stats_persist_failed",
            run_id = %run_id,
            session_id = %session_id,
            error = %e,
        );
    }
    session
        .add_usage(total_usage.input, total_usage.output)
        .await;
    let _ = session.save().await;

    let finished = RunEventContext::new(&run_id, &session_id, total_turns).finished(
        last_text,
        total_usage,
        total_turns,
        duration_ms,
        total_transcripts,
        compact_records,
    );
    let _ = tx.send(finished);
    drop(tx);

    tracing::info!(
        stage = "run",
        status = "finished",
        run_id = %run_id,
        session_id = %session_id,
        elapsed_ms = duration_ms,
        turn = total_turns,
    );

    if let Some(f) = on_complete {
        f();
    }
}

// ---------------------------------------------------------------------------
// drive_one_turn — single engine submit: forward events, persist transcripts
// ---------------------------------------------------------------------------

struct TurnOutcome {
    turn_count: u32,
    usage: UsageSummary,
    transcript_count: usize,
    last_text: String,
    /// Compact records produced during this engine turn, in order.
    compact_records: Vec<CompactRecord>,
    /// True if the engine reached `AgentEnd`; false on abort or channel close.
    engine_completed: bool,
    /// Transcript items produced during this engine turn.
    transcript: Vec<TranscriptItem>,
}

async fn drive_one_turn(
    turn: TurnInput,
    tx: &mpsc::UnboundedSender<RunEvent>,
    control: &RunControl,
    run_id: &str,
    session_id: &str,
    _started_at: Instant,
) -> TurnOutcome {
    let TurnInput {
        options,
        history,
        input,
        session,
        transcript_seq,
    } = turn;

    let mut engine = build_agent(options, history)
        .with_prompt_queues(control.queues().steering(), control.queues().follow_up());
    let user_msg = evot_engine::AgentMessage::Llm(evot_engine::Message::User {
        content: input.clone(),
        timestamp: evot_engine::now_ms(),
    });
    let (engine_handle, engine_rx) = engine.submit(vec![user_msg]).await;
    control.install_engine(engine_handle);

    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    let rid = run_id.to_string();
    let sid = session_id.to_string();
    tokio::spawn(async move {
        forward_events(engine_rx, runtime_tx, &rid, &sid).await;
    });

    // First user content is part of this turn's transcript record.
    let mut turn_transcripts: Vec<TranscriptItem> = vec![TranscriptItem::user_from_content(&input)];
    let mut saved_count: usize = 0;
    let mut expected_seq = transcript_seq;
    let mut transcript_rebased = false;
    let mut persistence_failed = false;
    let mut turn_count: u32 = 0;
    let mut outcome = TurnOutcome {
        turn_count: 0,
        usage: UsageSummary::default(),
        transcript_count: 0,
        last_text: String::new(),
        compact_records: Vec::new(),
        engine_completed: false,
        transcript: Vec::new(),
    };

    let flush = |session: &Arc<Session>,
                 items: &[TranscriptItem],
                 saved: &mut usize,
                 expected: &mut u64| {
        let new_items = items[*saved..].to_vec();
        let session = Arc::clone(session);
        let batch_len = new_items.len();
        let current_expected = *expected;
        async move {
            let next_seq = if new_items.is_empty() {
                current_expected
            } else {
                session.write_items_at(new_items, current_expected).await?
            };
            Ok::<(usize, u64), crate::error::EvotError>((batch_len, next_seq))
        }
    };

    while let Some(event) = runtime_rx.recv().await {
        match event {
            RuntimeEvent::TurnStarted => {
                turn_count = turn_count.saturating_add(1);
                session.increment_turn().await;
            }
            RuntimeEvent::Transcript(item) => {
                turn_transcripts.push(item);
            }
            RuntimeEvent::CompactionCompleted {
                reason,
                result,
                summary,
                messages,
                state,
                context_window,
                will_retry,
            } => {
                if let Some(record) = compact_record_from_result(&result) {
                    outcome.compact_records.push(record);
                }

                if matches!(result, crate::types::CompactionResult::Compacted { .. }) {
                    match flush(
                        &session,
                        &turn_transcripts,
                        &mut saved_count,
                        &mut expected_seq,
                    )
                    .await
                    {
                        Ok((written, next_seq)) => {
                            transcript_rebased |=
                                next_seq != expected_seq.saturating_add(written as u64);
                            saved_count = saved_count.saturating_add(written);
                            expected_seq = next_seq;
                        }
                        Err(error) => {
                            emit_persistence_error(tx, run_id, session_id, turn_count, &error);
                            control.abort();
                            persistence_failed = true;
                            break;
                        }
                    }

                    if transcript_rebased {
                        tracing::warn!(
                            stage = "run",
                            status = "compaction_skipped_after_rebase",
                            run_id,
                            session_id,
                            "skipping stale automatic compaction after concurrent transcript activity"
                        );
                    } else {
                        let mut new_context = from_agent_messages(&messages);
                        if let Some(summary) = summary.as_deref() {
                            for (index, message) in messages.iter().enumerate() {
                                if evot_engine::context::compaction::remote::is_replacement_message(
                                    message,
                                ) && index < new_context.len()
                                {
                                    new_context[index] =
                                        crate::compact::context_view::compact_summary_item(summary);
                                }
                            }
                        }
                        let item = automatic_compact_item(
                            reason.clone(),
                            &result,
                            summary.clone(),
                            new_context.clone(),
                            messages,
                            state,
                        );
                        if let Err(error) =
                            session.write_compact(item, new_context, expected_seq).await
                        {
                            emit_persistence_error(tx, run_id, session_id, turn_count, &error);
                            control.abort();
                            persistence_failed = true;
                            break;
                        }
                        expected_seq = expected_seq.saturating_add(1);
                    }
                }

                turn_transcripts.push(
                    TranscriptStats::ContextCompactionCompleted(ContextCompactionCompletedStats {
                        reason,
                        result,
                        context_window,
                        will_retry,
                    })
                    .to_item(),
                );
            }
            RuntimeEvent::TurnEnded => {
                match flush(
                    &session,
                    &turn_transcripts,
                    &mut saved_count,
                    &mut expected_seq,
                )
                .await
                {
                    Ok((written, next_seq)) => {
                        transcript_rebased |=
                            next_seq != expected_seq.saturating_add(written as u64);
                        saved_count = saved_count.saturating_add(written);
                        expected_seq = next_seq;
                    }
                    Err(error) => {
                        emit_persistence_error(tx, run_id, session_id, turn_count, &error);
                        control.abort();
                        persistence_failed = true;
                        break;
                    }
                }
            }
            RuntimeEvent::EngineCompleted {
                last_text,
                usage,
                transcript_count,
            } => {
                outcome.turn_count = turn_count;
                outcome.usage = usage;
                outcome.transcript_count = transcript_count;
                outcome.last_text = last_text;
                outcome.engine_completed = true;
                break;
            }
            RuntimeEvent::Public(payload) => {
                let event = RunEventContext::new(run_id, session_id, turn_count).event(payload);
                if tx.send(event).is_err() {
                    // Consumer dropped — bail out; outer loop will stop.
                    break;
                }
            }
        }
    }

    // Final flush in case engine ended without a TurnEnded event. A failed
    // strict write must not be retried against stale state.
    if !persistence_failed {
        if let Err(error) = flush(
            &session,
            &turn_transcripts,
            &mut saved_count,
            &mut expected_seq,
        )
        .await
        {
            emit_persistence_error(tx, run_id, session_id, turn_count, &error);
            control.abort();
        }
    }

    control.detach_engine();
    if outcome.turn_count == 0 {
        outcome.turn_count = turn_count;
    }
    outcome.transcript = turn_transcripts;
    outcome
}

// ---------------------------------------------------------------------------
// forward_events — AgentEvent → RuntimeEvent (one-step conversion)
// ---------------------------------------------------------------------------

async fn forward_events(
    mut engine_rx: mpsc::UnboundedReceiver<evot_engine::AgentEvent>,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    run_id: &str,
    session_id: &str,
) {
    while let Some(event) = engine_rx.recv().await {
        let runtime_events = map_agent_event(&event, run_id, session_id);
        for re in runtime_events {
            if tx.send(re).is_err() {
                break;
            }
        }
    }
}

/// Map a single `AgentEvent` to zero or more `RuntimeEvent`s.
fn map_agent_event(
    event: &evot_engine::AgentEvent,
    _run_id: &str,
    _session_id: &str,
) -> Vec<RuntimeEvent> {
    match event {
        evot_engine::AgentEvent::AgentStart => vec![],

        evot_engine::AgentEvent::QuotaWait { delay_ms, error } => {
            vec![RuntimeEvent::Public(RunEventPayload::QuotaWaiting {
                delay_ms: *delay_ms,
                error: error.clone(),
            })]
        }

        evot_engine::AgentEvent::OutageWait { delay_ms, error } => {
            vec![RuntimeEvent::Public(RunEventPayload::OutageWaiting {
                delay_ms: *delay_ms,
                error: error.clone(),
            })]
        }

        evot_engine::AgentEvent::AgentEnd { messages } => {
            let transcripts = from_agent_messages(messages);
            let usage = total_usage(messages);
            let transcript_count = messages.len();

            let last_text = transcripts
                .iter()
                .rev()
                .find_map(|t| {
                    if let TranscriptItem::Assistant { content, .. } = t {
                        let text = crate::types::assistant_text(content);
                        if !text.is_empty() {
                            return Some(text);
                        }
                    }
                    None
                })
                .unwrap_or_default();

            vec![RuntimeEvent::EngineCompleted {
                last_text,
                usage,
                transcript_count,
            }]
        }

        evot_engine::AgentEvent::TurnStart => {
            vec![
                RuntimeEvent::TurnStarted,
                RuntimeEvent::Public(RunEventPayload::TurnStarted {}),
            ]
        }

        evot_engine::AgentEvent::TurnEnd { .. } => {
            vec![RuntimeEvent::TurnEnded]
        }

        evot_engine::AgentEvent::MessageStart { .. } => vec![],

        evot_engine::AgentEvent::MessageUpdate {
            delta:
                evot_engine::StreamDelta::Text {
                    content_index,
                    delta,
                },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            content_index: *content_index,
            content_type: AssistantContentType::Text,
            delta: delta.clone(),
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta:
                evot_engine::StreamDelta::Thinking {
                    content_index,
                    delta,
                },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            content_index: *content_index,
            content_type: AssistantContentType::Thinking,
            delta: delta.clone(),
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta:
                evot_engine::StreamDelta::ToolCallStart {
                    content_index,
                    id,
                    name,
                },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantToolCall {
            content_index: *content_index,
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            phase: ToolCallStreamPhase::Start,
            delta: None,
            args: None,
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta:
                evot_engine::StreamDelta::ToolCallDelta {
                    content_index,
                    id,
                    name,
                    delta,
                },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantToolCall {
            content_index: *content_index,
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            phase: ToolCallStreamPhase::Delta,
            delta: Some(delta.clone()),
            args: None,
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta:
                evot_engine::StreamDelta::ToolCallEnd {
                    content_index,
                    id,
                    name,
                    arguments,
                },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantToolCall {
            content_index: *content_index,
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            phase: ToolCallStreamPhase::End,
            delta: None,
            args: Some(arguments.clone()),
        })],

        evot_engine::AgentEvent::MessageEnd { message } => {
            if let evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant {
                content,
                usage,
                stop_reason,
                error_message,
                ..
            }) = message
            {
                let blocks = assistant_blocks_from_content(content);
                let usage_summary = UsageSummary {
                    input: usage.input,
                    output: usage.output,
                    cache_read: usage.cache_read,
                    cache_write: usage.cache_write,
                };
                let transcript_item = transcript_from_agent_message(message);

                vec![
                    RuntimeEvent::Transcript(transcript_item),
                    RuntimeEvent::Public(RunEventPayload::AssistantCompleted {
                        content: blocks,
                        usage: Some(usage_summary),
                        stop_reason: stop_reason.to_string(),
                        error_message: error_message.clone(),
                    }),
                ]
            } else {
                vec![]
            }
        }

        evot_engine::AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            preview_command,
        } => vec![RuntimeEvent::Public(RunEventPayload::ToolStarted {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            args: args.clone(),
            preview_command: preview_command.clone(),
        })],

        evot_engine::AgentEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            partial_result,
        } => {
            let text = extract_content_text(&partial_result.content);
            vec![RuntimeEvent::Public(RunEventPayload::ToolProgress {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                text,
                details: partial_result.details.clone(),
            })]
        }

        evot_engine::AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
            result_tokens,
            duration_ms,
        } => {
            let content = extract_content_text(&result.content);
            vec![
                RuntimeEvent::Transcript(TranscriptItem::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                    details: result.details.clone(),
                }),
                RuntimeEvent::Transcript(
                    TranscriptStats::ToolFinished(ToolFinishedStats {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        result_tokens: *result_tokens,
                        duration_ms: *duration_ms,
                        is_error: *is_error,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ToolFinished {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content,
                    is_error: *is_error,
                    details: result.details.clone(),
                    result_tokens: *result_tokens,
                    duration_ms: *duration_ms,
                }),
            ]
        }

        evot_engine::AgentEvent::ProgressMessage {
            tool_call_id,
            tool_name,
            text,
        } => vec![RuntimeEvent::Public(RunEventPayload::ToolProgress {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            text: text.clone(),
            details: serde_json::Value::Null,
        })],

        evot_engine::AgentEvent::Error { error } => {
            vec![RuntimeEvent::Public(RunEventPayload::Error {
                message: error.message.clone(),
            })]
        }

        evot_engine::AgentEvent::LlmCallStart {
            turn,
            attempt,
            injected_count,
            request,
            stats,
            budget,
        } => {
            let message_count = request.messages.len();
            let tool_count = request.tools.len();

            // Compute message_bytes for transcript (still needs serialization)
            let message_bytes: usize = request
                .messages
                .iter()
                .map(|msg| serialize_or_placeholder(msg, "message").to_string().len())
                .sum();

            let message_stats = Some(LlmMessageStats::from(stats.clone()));

            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallStarted(LlmCallStartedStats {
                        turn: *turn,
                        attempt: *attempt,
                        injected_count: *injected_count,
                        model: request.model.clone(),
                        message_count,
                        message_bytes,
                        system_prompt_tokens: budget.system_prompt_tokens,
                        tool_definition_tokens: budget.tool_definition_tokens,
                        system_prompt: request.system_prompt.clone(),
                        tool_definitions: request
                            .tools
                            .iter()
                            .map(|t| ToolDef {
                                name: t.name.clone(),
                                description: t.description.clone(),
                                parameters: t.parameters.clone(),
                            })
                            .collect(),
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallStarted {
                    turn: *turn,
                    attempt: *attempt,
                    injected_count: *injected_count,
                    model: request.model.clone(),
                    message_count,
                    message_bytes,
                    estimated_context_tokens: budget.estimated_tokens,
                    system_prompt_tokens: budget.system_prompt_tokens,
                    tool_definition_tokens: budget.tool_definition_tokens,
                    tool_count,
                    message_stats,
                    budget_tokens: budget.budget_tokens,
                    context_window: budget.context_window,
                }),
            ]
        }

        evot_engine::AgentEvent::LlmCallRetry {
            turn,
            attempt,
            max_retries,
            delay_ms,
            error,
        } => {
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallRetry(LlmCallRetryStats {
                        turn: *turn,
                        attempt: *attempt,
                        max_retries: *max_retries,
                        delay_ms: *delay_ms,
                        error: error.clone(),
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallRetry {
                    turn: *turn,
                    attempt: *attempt,
                    max_retries: *max_retries,
                    delay_ms: *delay_ms,
                    error: error.clone(),
                }),
            ]
        }

        evot_engine::AgentEvent::LlmCallEnd {
            turn,
            attempt,
            usage,
            error,
            metrics,
            context_window,
            stop_reason,
            content,
            response_model,
            response_id: _,
        } => {
            let usage_summary = UsageSummary {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
            };
            let llm_metrics = LlmCallMetrics {
                duration_ms: metrics.duration_ms,
                ttfb_ms: metrics.ttfb_ms,
                ttft_ms: metrics.ttft_ms,
                streaming_ms: metrics.streaming_ms,
                chunk_count: metrics.chunk_count,
            };
            // Extract tool call summaries for the public event
            let tool_calls: Vec<LlmToolCallSummary> = content
                .iter()
                .filter_map(|c| match c {
                    evot_engine::Content::ToolCall {
                        id,
                        name,
                        arguments,
                        ..
                    } => Some(LlmToolCallSummary {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    }),
                    _ => None,
                })
                .collect();
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
                        turn: *turn,
                        attempt: *attempt,
                        usage: usage_summary.clone(),
                        metrics: Some(llm_metrics.clone()),
                        error: error.clone(),
                        context_window: *context_window,
                        stop_reason: stop_reason.to_string(),
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallCompleted {
                    turn: *turn,
                    attempt: *attempt,
                    usage: usage_summary.clone(),
                    cache_read: usage_summary.cache_read,
                    cache_write: usage_summary.cache_write,
                    error: error.clone(),
                    metrics: Some(llm_metrics),
                    context_window: *context_window,
                    stop_reason: stop_reason.to_string(),
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    response_model: response_model.clone(),
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionStarted {
            reason,
            estimated_tokens,
            context_window,
            reserve_tokens,
            trigger_threshold,
            will_retry,
        } => {
            let reason = map_compact_reason(*reason);
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::ContextCompactionStarted(ContextCompactionStartedStats {
                        reason: reason.clone(),
                        message_count: 0,
                        estimated_tokens: *estimated_tokens,
                        budget_tokens: context_window.saturating_sub(*reserve_tokens),
                        reserve_tokens: *reserve_tokens,
                        trigger_threshold: *trigger_threshold,
                        system_prompt_tokens: 0,
                        tool_definition_tokens: 0,
                        context_window: *context_window,
                        will_retry: *will_retry,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ContextCompactionStarted {
                    reason,
                    message_count: 0,
                    estimated_tokens: *estimated_tokens,
                    budget_tokens: context_window.saturating_sub(*reserve_tokens),
                    reserve_tokens: *reserve_tokens,
                    trigger_threshold: *trigger_threshold,
                    system_prompt_tokens: 0,
                    tool_definition_tokens: 0,
                    context_window: *context_window,
                    will_retry: *will_retry,
                    message_stats: None,
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionPhase { phase } => vec![RuntimeEvent::Public(
            RunEventPayload::ContextCompactionPhase { phase: *phase },
        )],

        evot_engine::AgentEvent::ContextCompactionEnd {
            reason,
            stats,
            messages,
            state,
            summary,
            context_window,
            will_retry,
        } => {
            let result = if stats.messages_evicted > 0 || stats.current_run_reclaimed > 0 {
                crate::types::CompactionResult::Compacted {
                    before_message_count: stats.before_message_count,
                    after_message_count: stats.after_message_count,
                    before_tokens: stats.before_tokens,
                    after_tokens: stats.after_tokens,
                    messages_evicted: stats.messages_evicted,
                    current_run_reclaimed: stats.current_run_reclaimed,
                    method: stats.method,
                    remote_blob_bytes: stats.remote_blob_bytes,
                    fallback_reason: stats.fallback_reason.clone(),
                }
            } else {
                crate::types::CompactionResult::NoOp
            };

            let reason = map_compact_reason(*reason);
            vec![
                RuntimeEvent::CompactionCompleted {
                    reason: reason.clone(),
                    result: result.clone(),
                    summary: summary.clone(),
                    messages: messages.clone(),
                    state: state.clone(),
                    context_window: *context_window,
                    will_retry: *will_retry,
                },
                RuntimeEvent::Public(RunEventPayload::ContextCompactionCompleted {
                    reason,
                    result,
                    summary: summary.clone(),
                    context_window: *context_window,
                    will_retry: *will_retry,
                }),
            ]
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn automatic_compact_item(
    reason: crate::types::CompactReason,
    result: &crate::types::CompactionResult,
    summary: Option<String>,
    messages: Vec<TranscriptItem>,
    engine_messages: Vec<evot_engine::AgentMessage>,
    state: evot_engine::CompactionState,
) -> TranscriptItem {
    let (messages_before, messages_after, tokens_before, tokens_after) = match result {
        crate::types::CompactionResult::Compacted {
            before_message_count,
            after_message_count,
            before_tokens,
            after_tokens,
            ..
        } => (
            *before_message_count,
            *after_message_count,
            *before_tokens,
            *after_tokens,
        ),
        crate::types::CompactionResult::NoOp => (0, 0, 0, 0),
    };
    let details = crate::types::CompactDetails {
        read_files: state.file_ops.read_only().into_iter().cloned().collect(),
        modified_files: state.file_ops.modified().into_iter().cloned().collect(),
        method: match result {
            crate::types::CompactionResult::Compacted { method, .. } => *method,
            crate::types::CompactionResult::NoOp => None,
        },
        remote_blob_bytes: match result {
            crate::types::CompactionResult::Compacted {
                remote_blob_bytes, ..
            } => *remote_blob_bytes,
            crate::types::CompactionResult::NoOp => None,
        },
        fallback_reason: match result {
            crate::types::CompactionResult::Compacted {
                fallback_reason, ..
            } => fallback_reason.clone(),
            crate::types::CompactionResult::NoOp => None,
        },
    };
    TranscriptItem::Compact {
        id: crate::types::new_id(),
        created_at: evot_engine::now_ms(),
        reason,
        summary: summary
            .or_else(|| state.last_summary.clone())
            .unwrap_or_default(),
        tokens_before,
        tokens_after,
        messages_before,
        messages_after,
        messages,
        engine_messages,
        state: Box::new(state),
        details,
    }
}

fn emit_persistence_error(
    tx: &mpsc::UnboundedSender<RunEvent>,
    run_id: &str,
    session_id: &str,
    turn: u32,
    error: &crate::error::EvotError,
) {
    tracing::error!(
        stage = "run",
        status = "persistence_failed",
        run_id,
        session_id,
        error = %error,
    );
    let _ = tx.send(
        RunEventContext::new(run_id, session_id, turn).event(RunEventPayload::Error {
            message: format!("session persistence failed: {error}"),
        }),
    );
}

fn map_compact_reason(reason: evot_engine::CompactReason) -> crate::types::CompactReason {
    match reason {
        evot_engine::CompactReason::Threshold => crate::types::CompactReason::Threshold,
        evot_engine::CompactReason::Overflow => crate::types::CompactReason::Overflow,
        evot_engine::CompactReason::Manual => crate::types::CompactReason::Manual,
    }
}

fn serialize_or_placeholder<T: serde::Serialize>(value: &T, kind: &str) -> serde_json::Value {
    match serde_json::to_value(value) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("failed to serialize {kind}: {e}");
            serde_json::json!({
                "type": "serialization_error",
                "kind": kind,
                "message": e.to_string(),
            })
        }
    }
}

fn compact_record_from_result(result: &crate::types::CompactionResult) -> Option<CompactRecord> {
    crate::agent::run::observability::compact_record_from_result(result)
}

pub(crate) fn build_agent(
    options: EngineOptions,
    prior_messages: Vec<evot_engine::AgentMessage>,
) -> evot_engine::Agent {
    use evot_engine::provider::AnthropicProvider;
    use evot_engine::provider::OpenAiCompatProvider;
    use evot_engine::provider::OpenAiResponsesProvider;

    let model_config = options.model_config;
    let context_config =
        evot_engine::context::ContextConfig::from_model(&model_config, options.thinking_level);

    let provider_agent = match (options.provider_override, &options.protocol) {
        (Some(provider), _) => evot_engine::Agent::new(provider),
        (None, Protocol::Anthropic) => evot_engine::Agent::new(AnthropicProvider),
        (None, Protocol::OpenAiResponses) => evot_engine::Agent::new(OpenAiResponsesProvider),
        (None, Protocol::OpenAi) => evot_engine::Agent::new(OpenAiCompatProvider),
    };

    let limits = options
        .limits
        .map(|l| evot_engine::context::ExecutionLimits {
            max_turns: l.max_turns as usize,
            max_total_tokens: l.max_total_tokens as usize,
            max_duration: std::time::Duration::from_secs(l.max_duration_secs),
        });

    let agent = provider_agent
        .with_model(&options.model)
        .with_api_key(&options.api_key)
        .with_model_config(model_config)
        .with_context_config(context_config)
        .with_system_prompt(&options.system_prompt)
        .with_messages(prior_messages)
        .with_execution_limits_opt(limits)
        .with_tools(options.tools)
        .with_cwd(options.cwd)
        .with_path_guard(options.path_guard)
        .with_thinking(options.thinking_level)
        .with_compaction_state_opt(options.compaction_state)
        .with_prompt_cache_key_opt(options.prompt_cache_key)
        .with_spill_opt(
            options
                .spill_dir
                .map(|dir| Arc::new(evot_engine::spill::FsSpill::new(dir))),
        );
    match options.process_manager {
        Some(process_manager) => agent.with_process_manager(process_manager),
        None => agent,
    }
}
