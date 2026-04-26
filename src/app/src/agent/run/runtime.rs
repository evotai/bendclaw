//! Engine runtime — create engine, forward events, orchestrate a run.
//!
//! This module owns the boundary between `evot_engine::AgentEvent` and the
//! app-layer `RunEvent`. No engine types leak beyond this module.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use super::convert::assistant_blocks_from_content;
use super::convert::extract_content_text;
use super::convert::from_agent_messages;
use super::convert::total_usage;
use super::convert::transcript_from_assistant_completed;
use super::event::LlmMessageStats;
use super::event::RunEvent;
use super::event::RunEventContext;
use super::event::RunEventPayload;
use super::run::Run;
use crate::agent::session::Session;
use crate::conf::Protocol;
use crate::error::Result;
use crate::types::CompactRecord;
use crate::types::ContextCompactionCompletedStats;
use crate::types::ContextCompactionStartedStats;
use crate::types::LlmCallCompletedStats;
use crate::types::LlmCallMetrics;
use crate::types::LlmCallStartedStats;
use crate::types::RunFinishedStats;
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
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub limits: crate::agent::ExecutionLimits,
    pub skills_dirs: Vec<std::path::PathBuf>,
    pub tools: Vec<Box<dyn evot_engine::AgentTool>>,
    pub thinking_level: evot_engine::ThinkingLevel,
    pub compat_caps: evot_engine::provider::CompatCaps,
    pub cwd: std::path::PathBuf,
    pub path_guard: std::sync::Arc<evot_engine::PathGuard>,
}

// ---------------------------------------------------------------------------
// TurnInput — prepared by agent, executed by runtime
// ---------------------------------------------------------------------------

pub(in crate::agent) struct TurnInput {
    pub options: EngineOptions,
    pub history: Vec<evot_engine::AgentMessage>,
    pub input: Vec<evot_engine::Content>,
    pub session: Arc<Session>,
    pub run_id: String,
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// execute_turn — build engine, submit, forward events, persist transcript
// ---------------------------------------------------------------------------

pub(in crate::agent) async fn execute_turn(
    turn: TurnInput,
    on_complete: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<Run> {
    let mut engine = build_agent(turn.options, turn.history);
    let user_msg = evot_engine::AgentMessage::Llm(evot_engine::Message::User {
        content: turn.input.clone(),
        timestamp: evot_engine::now_ms(),
    });
    let (run_handle, engine_rx) = engine.submit(vec![user_msg]).await;

    let (runtime_tx, runtime_rx) = mpsc::unbounded_channel();

    let rid = turn.run_id.clone();
    let sid = turn.session_id.clone();
    tokio::spawn(async move {
        forward_events(engine_rx, runtime_tx, &rid, &sid).await;
    });

    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(run_loop(
        runtime_rx,
        tx,
        turn.session,
        turn.input,
        turn.run_id.clone(),
        turn.session_id.clone(),
        on_complete,
    ));

    Ok(Run::new(turn.run_id, turn.session_id, rx, run_handle))
}

// ---------------------------------------------------------------------------
// RuntimeEvent — private orchestration signal
// ---------------------------------------------------------------------------

enum RuntimeEvent {
    Public(RunEventPayload),
    Transcript(TranscriptItem),
    TurnStarted,
    TurnEnded,
    RunCompleted {
        last_text: String,
        usage: UsageSummary,
        transcript_count: usize,
    },
    Compacted {
        level: u8,
        transcripts: Vec<TranscriptItem>,
    },
}

// ---------------------------------------------------------------------------
// run_loop — orchestrate a single run (transcript persistence + event relay)
// ---------------------------------------------------------------------------

async fn run_loop(
    mut rx: mpsc::UnboundedReceiver<RuntimeEvent>,
    tx: mpsc::UnboundedSender<RunEvent>,
    session: Arc<Session>,
    input: Vec<evot_engine::Content>,
    run_id: String,
    session_id: String,
    on_complete: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    let started_at = Instant::now();
    let ctx = RunEventContext::new(&run_id, &session_id, 0);

    // Send RunStarted
    let _ = tx.send(ctx.started());

    let mut run_transcripts: Vec<TranscriptItem> = vec![TranscriptItem::user_from_content(&input)];
    let mut saved_count: usize = 0;
    let mut turn = 0_u32;
    let mut got_run_completed = false;

    // Flush unsaved transcript items to storage.
    let flush = |session: &Arc<Session>, transcripts: &[TranscriptItem], saved: &mut usize| {
        let new_items = transcripts[*saved..].to_vec();
        let session = Arc::clone(session);
        *saved = transcripts.len();
        async move {
            if !new_items.is_empty() {
                session.write_items(new_items).await
            } else {
                Ok(())
            }
        }
    };

    while let Some(event) = rx.recv().await {
        match event {
            RuntimeEvent::TurnStarted => {
                turn += 1;
                session.increment_turn().await;
            }
            RuntimeEvent::Transcript(item) => {
                run_transcripts.push(item);
            }
            RuntimeEvent::Compacted { level, transcripts } => {
                if level > 0 {
                    run_transcripts.push(TranscriptItem::Marker {
                        kind: crate::types::MarkerKind::Compact,
                        target_seq: None,
                        messages: transcripts,
                    });
                }
            }
            RuntimeEvent::TurnEnded => {
                if let Err(e) = flush(&session, &run_transcripts, &mut saved_count).await {
                    tracing::error!(
                        stage = "run",
                        status = "incremental_save_failed",
                        run_id = %run_id,
                        session_id = %session_id,
                        error = %e,
                    );
                }
            }
            RuntimeEvent::RunCompleted {
                last_text,
                usage,
                transcript_count,
            } => {
                got_run_completed = true;

                let duration_ms = started_at.elapsed().as_millis() as u64;
                let stats = TranscriptStats::RunFinished(RunFinishedStats {
                    usage: usage.clone(),
                    turn_count: turn,
                    duration_ms,
                    transcript_count,
                });
                run_transcripts.push(stats.to_item());

                // Extract compact history from transcripts
                let compact_history = extract_compact_history(&run_transcripts);

                let finished_event = RunEventContext::new(&run_id, &session_id, turn).finished(
                    last_text,
                    usage,
                    turn,
                    duration_ms,
                    transcript_count,
                    compact_history,
                );
                let _ = tx.send(finished_event);
                // Drop tx immediately so the consumer stream closes without
                // waiting for transcript persistence.
                drop(tx);
                break;
            }
            RuntimeEvent::Public(payload) => {
                let event = RunEventContext::new(&run_id, &session_id, turn).event(payload);
                if tx.send(event).is_err() {
                    break;
                }
            }
        }
    }

    // Fallback save
    if !got_run_completed {
        let _ = flush(&session, &run_transcripts, &mut saved_count).await;
    }

    let _ = session.save().await;

    tracing::info!(
        stage = "run",
        status = "finished",
        run_id = %run_id,
        session_id = %session_id,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        turn,
    );

    if let Some(f) = on_complete {
        f();
    }
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
                return;
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

        evot_engine::AgentEvent::AgentEnd { messages } => {
            let transcripts = from_agent_messages(messages);
            let usage = total_usage(messages);
            let transcript_count = messages.len();

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

            vec![RuntimeEvent::RunCompleted {
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
            delta: evot_engine::StreamDelta::Text { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: Some(delta.clone()),
            thinking_delta: None,
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta: evot_engine::StreamDelta::Thinking { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: None,
            thinking_delta: Some(delta.clone()),
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta: evot_engine::StreamDelta::ToolCallDelta { .. },
            ..
        } => vec![],

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
                let transcript_item =
                    transcript_from_assistant_completed(&blocks, &stop_reason.to_string());

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

            // Convert engine LlmCallStats → app LlmMessageStats
            let message_stats = Some(LlmMessageStats {
                user_count: stats.user_count,
                assistant_count: stats.assistant_count,
                tool_result_count: stats.tool_result_count,
                image_count: stats.image_count,
                user_tokens: stats.user_tokens,
                assistant_tokens: stats.assistant_tokens,
                tool_result_tokens: stats.tool_result_tokens,
                image_tokens: stats.image_tokens,
                tool_details: stats.tool_details.clone(),
            });

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
                    tool_count,
                    message_stats,
                    budget_tokens: budget.budget_tokens,
                    context_window: budget.context_window,
                }),
            ]
        }

        evot_engine::AgentEvent::LlmCallEnd {
            turn,
            attempt,
            usage,
            error,
            metrics,
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
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
                        turn: *turn,
                        attempt: *attempt,
                        usage: usage_summary.clone(),
                        metrics: Some(llm_metrics.clone()),
                        error: error.clone(),
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
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionStart {
            message_count,
            budget,
            message_stats,
        } => {
            let stats = Some(LlmMessageStats {
                user_count: message_stats.user_count,
                assistant_count: message_stats.assistant_count,
                tool_result_count: message_stats.tool_result_count,
                image_count: message_stats.image_count,
                user_tokens: message_stats.user_tokens,
                assistant_tokens: message_stats.assistant_tokens,
                tool_result_tokens: message_stats.tool_result_tokens,
                image_tokens: message_stats.image_tokens,
                tool_details: message_stats.tool_details.clone(),
            });
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::ContextCompactionStarted(ContextCompactionStartedStats {
                        message_count: *message_count,
                        estimated_tokens: budget.estimated_tokens,
                        budget_tokens: budget.budget_tokens,
                        system_prompt_tokens: budget.system_prompt_tokens,
                        context_window: budget.context_window,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ContextCompactionStarted {
                    message_count: *message_count,
                    estimated_tokens: budget.estimated_tokens,
                    budget_tokens: budget.budget_tokens,
                    system_prompt_tokens: budget.system_prompt_tokens,
                    context_window: budget.context_window,
                    message_stats: stats,
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionEnd { stats, messages } => {
            let compacted_transcripts = from_agent_messages(messages);

            let result = if stats.level > 0 {
                crate::types::CompactionResult::LevelCompacted {
                    level: stats.level,
                    before_message_count: stats.before_message_count,
                    after_message_count: stats.after_message_count,
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    tool_outputs_truncated: stats.tool_outputs_truncated,
                    turns_summarized: stats.turns_summarized,
                    messages_dropped: stats.messages_dropped,
                    oversize_capped: stats.oversize_capped,
                    age_cleared: stats.age_cleared,
                    actions: stats
                        .actions
                        .iter()
                        .map(|a| crate::types::CompactionAction {
                            index: a.index,
                            tool_name: a.tool_name.clone(),
                            method: format!("{:?}", a.method),
                            before_tokens: a.before_tokens,
                            after_tokens: a.after_tokens,
                            end_index: a.end_index,
                            related_count: a.related_count,
                        })
                        .collect(),
                }
            } else if stats.current_run_cleared > 0 {
                crate::types::CompactionResult::RunOnceCleared {
                    cleared_count: stats.current_run_cleared,
                    before_message_count: stats.before_message_count,
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    saved_tokens: stats
                        .before_estimated_tokens
                        .saturating_sub(stats.after_estimated_tokens),
                    actions: stats
                        .actions
                        .iter()
                        .map(|a| crate::types::CompactionAction {
                            index: a.index,
                            tool_name: a.tool_name.clone(),
                            method: format!("{:?}", a.method),
                            before_tokens: a.before_tokens,
                            after_tokens: a.after_tokens,
                            end_index: a.end_index,
                            related_count: a.related_count,
                        })
                        .collect(),
                }
            } else {
                crate::types::CompactionResult::NoOp
            };

            vec![
                RuntimeEvent::Compacted {
                    level: stats.level,
                    transcripts: compacted_transcripts,
                },
                RuntimeEvent::Transcript(
                    TranscriptStats::ContextCompactionCompleted(ContextCompactionCompletedStats {
                        result: result.clone(),
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ContextCompactionCompleted { result }),
            ]
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn extract_compact_history(transcripts: &[TranscriptItem]) -> Vec<CompactRecord> {
    use crate::agent::run::observability::compact_record_from_result;

    transcripts
        .iter()
        .filter_map(|item| {
            let stats = TranscriptStats::try_from_item(item)?;
            match stats {
                TranscriptStats::ContextCompactionCompleted(s) => {
                    compact_record_from_result(&s.result)
                }
                _ => None,
            }
        })
        .collect()
}

pub(crate) fn build_agent(
    options: EngineOptions,
    prior_messages: Vec<evot_engine::AgentMessage>,
) -> evot_engine::Agent {
    use evot_engine::provider::AnthropicProvider;
    use evot_engine::provider::ModelConfig;
    use evot_engine::provider::OpenAiCompat;
    use evot_engine::provider::OpenAiCompatProvider;

    let mut model_config = match options.protocol {
        Protocol::Anthropic => ModelConfig::anthropic(&options.model, &options.model),
        Protocol::OpenAi => {
            let mut mc = ModelConfig::local("", &options.model);
            mc.compat = Some(match options.provider.as_str() {
                "openai" => OpenAiCompat::openai(),
                "deepseek" => OpenAiCompat::deepseek(),
                "xai" => OpenAiCompat::xai(),
                "groq" => OpenAiCompat::groq(),
                "cerebras" => OpenAiCompat::cerebras(),
                "openrouter" => OpenAiCompat::openrouter(),
                "mistral" => OpenAiCompat::mistral(),
                "zai" => OpenAiCompat::zai(),
                "minimax" => OpenAiCompat::minimax(),
                _ => OpenAiCompat::default(),
            });
            mc
        }
    };
    if let Some(base_url) = &options.base_url {
        model_config.base_url = base_url.clone();
    }

    if options.protocol == Protocol::OpenAi {
        if let Some(compat) = &mut model_config.compat {
            compat.caps |= options.compat_caps;
        }
    }

    let provider_agent = match options.protocol {
        Protocol::Anthropic => evot_engine::Agent::new(AnthropicProvider),
        Protocol::OpenAi => evot_engine::Agent::new(OpenAiCompatProvider),
    };

    let limits = evot_engine::context::ExecutionLimits {
        max_turns: options.limits.max_turns as usize,
        max_total_tokens: options.limits.max_total_tokens as usize,
        max_duration: std::time::Duration::from_secs(options.limits.max_duration_secs),
    };

    let skills = if options.skills_dirs.is_empty() {
        evot_engine::SkillSet::empty()
    } else {
        match crate::agent::prompt::skill::load_skills(&options.skills_dirs) {
            Ok(specs) => evot_engine::SkillSet::new(specs),
            Err(e) => {
                tracing::warn!("failed to load skills: {e}");
                evot_engine::SkillSet::empty()
            }
        }
    };

    provider_agent
        .with_model(&options.model)
        .with_api_key(&options.api_key)
        .with_model_config(model_config)
        .with_system_prompt(&options.system_prompt)
        .with_messages(prior_messages)
        .with_execution_limits(limits)
        .with_tools(options.tools)
        .with_cwd(options.cwd)
        .with_path_guard(options.path_guard)
        .with_skills(skills)
        .with_thinking(options.thinking_level)
}
