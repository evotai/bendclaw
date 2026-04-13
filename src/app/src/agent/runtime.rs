//! Engine runtime — create engine, forward events, orchestrate a run.
//!
//! This module owns the boundary between `bend_engine::AgentEvent` and the
//! app-layer `RunEvent`. No engine types leak beyond this module.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use super::convert::assistant_blocks_from_content;
use super::convert::extract_content_text;
use super::convert::from_agent_messages;
use super::convert::into_agent_messages;
use super::convert::total_usage;
use super::convert::transcript_from_assistant_completed;
use super::event::RunEvent;
use super::event::RunEventContext;
use super::event::RunEventPayload;
use crate::conf::ProviderKind;
use crate::error::Result;
use crate::session::Session;
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
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub limits: super::ExecutionLimits,
    pub skills_dirs: Vec<std::path::PathBuf>,
    pub tools: Vec<Box<dyn bend_engine::AgentTool>>,
}

/// Handle to a running engine instance.
/// Provides abort capability.
pub struct EngineHandle {
    agent: Option<bend_engine::Agent>,
}

impl EngineHandle {
    /// Abort the engine run.
    pub fn abort(&self) {
        if let Some(agent) = self.agent.as_ref() {
            agent.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// RuntimeEvent — private orchestration signal
// ---------------------------------------------------------------------------

/// Internal event produced by the forwarder. Not exported outside the agent module.
/// Carries both public events (for the consumer) and orchestration signals
/// (for transcript persistence and run lifecycle).
pub(super) enum RuntimeEvent {
    /// Forward to the external consumer as a RunEvent.
    Public(RunEventPayload),
    /// A transcript item was produced (for persistence).
    Transcript(TranscriptItem),
    /// A turn started.
    TurnStarted,
    /// A turn ended (flush transcripts).
    TurnEnded,
    /// The agent loop finished.
    RunCompleted {
        last_text: String,
        usage: UsageSummary,
        transcript_count: usize,
    },
    /// Context was compacted at the given level.
    Compacted {
        level: u8,
        transcripts: Vec<TranscriptItem>,
    },
}

// ---------------------------------------------------------------------------
// create_engine — build and start the engine
// ---------------------------------------------------------------------------

/// Build a `bend_engine::Agent`, start it with the given prompt, and spawn
/// a forwarder task that converts `AgentEvent` → `RuntimeEvent`.
///
/// Returns a `RuntimeEvent` receiver and an `EngineHandle` for abort.
pub(super) async fn create_engine(
    options: EngineOptions,
    prior_transcripts: &[TranscriptItem],
    prompt: String,
    run_id: &str,
    session_id: &str,
) -> Result<(mpsc::UnboundedReceiver<RuntimeEvent>, EngineHandle)> {
    let prior_messages = into_agent_messages(prior_transcripts);
    let prior_messages = bend_engine::sanitize_tool_pairs(prior_messages);
    let mut agent = build_agent(options, prior_messages);
    let engine_rx = agent.prompt(prompt).await;

    let (tx, rx) = mpsc::unbounded_channel();

    let rid = run_id.to_string();
    let sid = session_id.to_string();
    tokio::spawn(async move {
        forward_events(engine_rx, tx, &rid, &sid).await;
    });

    let handle = EngineHandle { agent: Some(agent) };
    Ok((rx, handle))
}

// ---------------------------------------------------------------------------
// run_loop — orchestrate a single run (transcript persistence + event relay)
// ---------------------------------------------------------------------------

/// Consume `RuntimeEvent`s, persist transcripts, and relay `RunEvent`s to the
/// external consumer. This is the run orchestration loop extracted from
/// `Agent::query()`.
pub(super) async fn run_loop(
    mut rx: mpsc::UnboundedReceiver<RuntimeEvent>,
    tx: mpsc::UnboundedSender<RunEvent>,
    session: Arc<Session>,
    prompt: String,
    run_id: String,
    session_id: String,
) {
    let started_at = Instant::now();
    let ctx = RunEventContext::new(&run_id, &session_id, 0);

    // Send RunStarted
    let _ = tx.send(ctx.started());

    let mut run_transcripts: Vec<TranscriptItem> = vec![TranscriptItem::User { text: prompt }];
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
                    run_transcripts.push(TranscriptItem::Compact {
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

                // Push RunFinished stats BEFORE flush so it gets persisted.
                let duration_ms = started_at.elapsed().as_millis() as u64;
                let stats = TranscriptStats::RunFinished(RunFinishedStats {
                    usage: usage.clone(),
                    turn_count: turn,
                    duration_ms,
                    transcript_count,
                });
                run_transcripts.push(stats.to_item());

                if let Err(e) = flush(&session, &run_transcripts, &mut saved_count).await {
                    tracing::error!(
                        stage = "run",
                        status = "transcript_save_failed",
                        run_id = %run_id,
                        session_id = %session_id,
                        error = %e,
                    );
                }

                let finished_event = RunEventContext::new(&run_id, &session_id, turn).finished(
                    last_text,
                    usage,
                    turn,
                    duration_ms,
                    transcript_count,
                );
                let _ = tx.send(finished_event);
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
}

// ---------------------------------------------------------------------------
// forward_events — AgentEvent → RuntimeEvent (one-step conversion)
// ---------------------------------------------------------------------------

async fn forward_events(
    mut engine_rx: mpsc::UnboundedReceiver<bend_engine::AgentEvent>,
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
    event: &bend_engine::AgentEvent,
    _run_id: &str,
    _session_id: &str,
) -> Vec<RuntimeEvent> {
    match event {
        bend_engine::AgentEvent::AgentStart => vec![],

        bend_engine::AgentEvent::AgentEnd { messages } => {
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

        bend_engine::AgentEvent::TurnStart => {
            vec![
                RuntimeEvent::TurnStarted,
                RuntimeEvent::Public(RunEventPayload::TurnStarted {}),
            ]
        }

        bend_engine::AgentEvent::TurnEnd { .. } => {
            vec![RuntimeEvent::TurnEnded]
        }

        bend_engine::AgentEvent::MessageStart { .. } => vec![],

        bend_engine::AgentEvent::MessageUpdate {
            delta: bend_engine::StreamDelta::Text { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: Some(delta.clone()),
            thinking_delta: None,
        })],

        bend_engine::AgentEvent::MessageUpdate {
            delta: bend_engine::StreamDelta::Thinking { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: None,
            thinking_delta: Some(delta.clone()),
        })],

        bend_engine::AgentEvent::MessageUpdate {
            delta: bend_engine::StreamDelta::ToolCallDelta { .. },
            ..
        } => vec![],

        bend_engine::AgentEvent::MessageEnd { message } => {
            if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
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

        bend_engine::AgentEvent::ToolExecutionStart {
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

        bend_engine::AgentEvent::ToolExecutionUpdate {
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

        bend_engine::AgentEvent::ToolExecutionEnd {
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

        bend_engine::AgentEvent::ProgressMessage {
            tool_call_id,
            tool_name,
            text,
        } => vec![RuntimeEvent::Public(RunEventPayload::ToolProgress {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            text: text.clone(),
        })],

        bend_engine::AgentEvent::Error { error } => {
            vec![RuntimeEvent::Public(RunEventPayload::Error {
                message: error.message.clone(),
            })]
        }

        bend_engine::AgentEvent::LlmCallStart {
            turn,
            attempt,
            request,
        } => {
            let message_count = request.messages.len();
            let system_prompt_tokens =
                bend_engine::context::estimate_tokens(&request.system_prompt);
            let messages: Vec<serde_json::Value> = request
                .messages
                .iter()
                .map(|m| serialize_or_placeholder(m, "message"))
                .collect();
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| serialize_or_placeholder(t, "tool"))
                .collect();
            let message_bytes: usize = messages.iter().map(|m| m.to_string().len()).sum();
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallStarted(LlmCallStartedStats {
                        turn: *turn,
                        attempt: *attempt,
                        model: request.model.clone(),
                        message_count,
                        message_bytes,
                        system_prompt_tokens,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallStarted {
                    turn: *turn,
                    attempt: *attempt,
                    model: request.model.clone(),
                    system_prompt: request.system_prompt.clone(),
                    messages,
                    tools,
                    message_count,
                    message_bytes,
                    system_prompt_tokens,
                }),
            ]
        }

        bend_engine::AgentEvent::LlmCallEnd {
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

        bend_engine::AgentEvent::ContextCompactionStart {
            message_count,
            estimated_tokens,
            budget_tokens,
            system_prompt_tokens,
            context_window,
        } => vec![
            RuntimeEvent::Transcript(
                TranscriptStats::ContextCompactionStarted(ContextCompactionStartedStats {
                    message_count: *message_count,
                    estimated_tokens: *estimated_tokens,
                    budget_tokens: *budget_tokens,
                    system_prompt_tokens: *system_prompt_tokens,
                    context_window: *context_window,
                })
                .to_item(),
            ),
            RuntimeEvent::Public(RunEventPayload::ContextCompactionStarted {
                message_count: *message_count,
                estimated_tokens: *estimated_tokens,
                budget_tokens: *budget_tokens,
                system_prompt_tokens: *system_prompt_tokens,
                context_window: *context_window,
            }),
        ],

        bend_engine::AgentEvent::ContextCompactionEnd { stats, messages } => {
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
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    saved_tokens: stats
                        .before_estimated_tokens
                        .saturating_sub(stats.after_estimated_tokens),
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

pub(crate) fn build_agent(
    options: EngineOptions,
    prior_messages: Vec<bend_engine::AgentMessage>,
) -> bend_engine::Agent {
    use bend_engine::provider::AnthropicProvider;
    use bend_engine::provider::ModelConfig;
    use bend_engine::provider::OpenAiCompatProvider;

    let mut model_config = match options.provider {
        ProviderKind::Anthropic => ModelConfig::anthropic(&options.model, &options.model),
        ProviderKind::OpenAi => ModelConfig::openai(&options.model, &options.model),
    };
    if let Some(base_url) = &options.base_url {
        model_config.base_url = base_url.clone();
    }

    let provider_agent = match options.provider {
        ProviderKind::Anthropic => bend_engine::Agent::new(AnthropicProvider),
        ProviderKind::OpenAi => bend_engine::Agent::new(OpenAiCompatProvider),
    };

    let limits = bend_engine::context::ExecutionLimits {
        max_turns: options.limits.max_turns as usize,
        max_total_tokens: options.limits.max_total_tokens as usize,
        max_duration: std::time::Duration::from_secs(options.limits.max_duration_secs),
    };

    let skills = if options.skills_dirs.is_empty() {
        bend_engine::SkillSet::empty()
    } else {
        match crate::agent::prompt::skill::load_skills(&options.skills_dirs) {
            Ok(specs) => bend_engine::SkillSet::new(specs),
            Err(e) => {
                tracing::warn!("failed to load skills: {e}");
                bend_engine::SkillSet::empty()
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
        .with_skills(skills)
}
