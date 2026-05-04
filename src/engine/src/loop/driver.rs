//! The core agent loop: prompt → LLM stream → tool execution → repeat.
//!
//! - `agent_loop()` starts with new prompt messages
//! - `agent_loop_continue()` resumes from existing context
//!
//! Both return a stream of `AgentEvent`s.

use tokio::sync::mpsc;

use super::compaction::compact_context;
use super::compaction::compact_for_recovery;
use super::config::AgentLoopConfig;
use super::doom_loop::DoomLoopDetector;
use super::input_filter::apply_input_filters;
use super::llm_call::stream_assistant_response;
use super::tool_exec::execute_tool_calls;
use super::tool_exec::skip_tool_call_doom_loop;
use crate::context::ContextTracker;
use crate::context::ExecutionTracker;
use crate::context::{self};
use crate::provider::ToolDefinition;
use crate::types::*;

/// Start an agent loop with new prompt messages.
pub async fn agent_loop(
    prompts: Vec<AgentMessage>,
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Vec<AgentMessage> {
    tx.send(AgentEvent::AgentStart).ok();

    // Apply input filters
    let prompts = match apply_input_filters(prompts, &config.input_filters, &tx) {
        Some(p) => p,
        None => return vec![],
    };

    let mut new_messages: Vec<AgentMessage> = prompts.clone();

    // Add prompts to context
    for prompt in &prompts {
        context.messages.push(prompt.clone());
    }

    tx.send(AgentEvent::TurnStart).ok();

    // Emit events for each prompt message
    for prompt in &prompts {
        tx.send(AgentEvent::MessageStart {
            message: prompt.clone(),
        })
        .ok();
        tx.send(AgentEvent::MessageEnd {
            message: prompt.clone(),
        })
        .ok();
    }

    run_loop(context, &mut new_messages, config, &tx, &cancel).await;

    tx.send(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .ok();
    new_messages
}

/// Continue an agent loop from existing context (for retries).
pub async fn agent_loop_continue(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Vec<AgentMessage> {
    tx.send(AgentEvent::AgentStart).ok();

    if context.messages.is_empty() {
        tx.send(AgentEvent::Error {
            error: AgentErrorInfo {
                kind: AgentErrorKind::Runtime,
                message: "Cannot continue: no messages in context".into(),
            },
        })
        .ok();
        tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
        return vec![];
    }

    if let Some(last) = context.messages.last() {
        if last.role() == "assistant" {
            tx.send(AgentEvent::Error {
                error: AgentErrorInfo {
                    kind: AgentErrorKind::Runtime,
                    message: "Cannot continue from assistant message".into(),
                },
            })
            .ok();
            tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
            return vec![];
        }
    }

    let mut new_messages: Vec<AgentMessage> = Vec::new();

    tx.send(AgentEvent::TurnStart).ok();

    run_loop(context, &mut new_messages, config, &tx, &cancel).await;

    tx.send(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .ok();
    new_messages
}

/// Main loop logic shared by agent_loop and agent_loop_continue.
///
/// Outer loop: continues when follow-up messages arrive after agent would stop.
/// Inner loop: process tool calls and steering messages.
async fn run_loop(
    context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
) {
    let mut first_turn = true;
    let mut turn_number: usize = 0;
    let mut tracker = config
        .execution_limits
        .as_ref()
        .map(|limits| ExecutionTracker::new(limits.clone()));
    let mut doom_detector = DoomLoopDetector::new(3);
    let mut context_tracker = ContextTracker::new();
    let mut consecutive_errors: usize = 0;
    let mut compacted_after_error = false;

    // Check for steering messages at start
    let mut pending: Vec<AgentMessage> = config
        .get_steering_messages
        .as_ref()
        .map(|f| f())
        .unwrap_or_default();

    // Outer loop: follow-ups after agent would stop
    loop {
        if cancel.is_cancelled() {
            return;
        }

        let mut steering_after_tools: Option<Vec<AgentMessage>> = None;

        // Inner loop: runs at least once, then continues if tool calls or pending messages
        loop {
            if cancel.is_cancelled() {
                return;
            }

            if !first_turn {
                tx.send(AgentEvent::TurnStart).ok();
            } else {
                first_turn = false;
            }

            // Inject pending messages (steering / follow-up / initial prompt)
            let injected_count = pending.len();
            if !pending.is_empty() {
                for msg in pending.drain(..) {
                    tx.send(AgentEvent::MessageStart {
                        message: msg.clone(),
                    })
                    .ok();
                    tx.send(AgentEvent::MessageEnd {
                        message: msg.clone(),
                    })
                    .ok();
                    context.messages.push(msg.clone());
                    new_messages.push(msg);
                }
            }

            // Check execution limits
            if let Some(ref tracker) = tracker {
                if let Some(reason) = tracker.check_limits() {
                    let limit_msg = AgentMessage::Llm(Message::User {
                        content: vec![Content::Text {
                            text: format!("[Agent stopped: {}]", reason),
                        }],
                        timestamp: now_ms(),
                    });
                    tx.send(AgentEvent::MessageStart {
                        message: limit_msg.clone(),
                    })
                    .ok();
                    tx.send(AgentEvent::MessageEnd {
                        message: limit_msg.clone(),
                    })
                    .ok();
                    context.messages.push(limit_msg.clone());
                    new_messages.push(limit_msg);
                    return;
                }
            }

            // before_turn callback — abort if it returns false
            if let Some(ref before_turn) = config.before_turn {
                if !before_turn(&context.messages, turn_number) {
                    return;
                }
            }
            turn_number += 1;

            // Compact context if configured
            compact_context(context, config, &mut context_tracker, tx);

            // Build budget snapshot for the LLM call (same source as compaction)
            let tool_defs: Vec<ToolDefinition> = context
                .tools
                .iter()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters_schema(),
                })
                .collect();
            context_tracker.record_request_overhead(&context.system_prompt, &tool_defs);
            let budget_snapshot =
                context_tracker.budget_snapshot(&context.messages, config.context_config.as_ref());

            // Stream assistant response
            let message = stream_assistant_response(
                context,
                config,
                tx,
                cancel,
                turn_number,
                injected_count,
                budget_snapshot,
            )
            .await;

            let agent_msg: AgentMessage = message.clone().into();
            context.messages.push(agent_msg.clone());
            new_messages.push(agent_msg.clone());

            // Record real usage from provider for accurate context tracking
            if let Message::Assistant { ref usage, .. } = message {
                let msg_index = context.messages.len() - 1;
                context_tracker.record_usage(usage, msg_index);
            }

            // Check for error/abort
            if let Message::Assistant {
                ref stop_reason,
                ref error_message,
                ref usage,
                ..
            } = message
            {
                if *stop_reason == StopReason::Error || *stop_reason == StopReason::Aborted {
                    consecutive_errors += 1;

                    // Compact-and-retry: if we've failed ≥2 times consecutively,
                    // context is over 50% of budget, and we haven't already
                    // compacted for this error streak — force compact and retry.
                    if *stop_reason == StopReason::Error
                        && !cancel.is_cancelled()
                        && consecutive_errors >= 2
                        && !compacted_after_error
                        && compact_for_recovery(
                            context,
                            new_messages,
                            config,
                            &mut context_tracker,
                            tx,
                        )
                    {
                        compacted_after_error = true;
                        continue;
                    }

                    // Emit unified Error event for provider errors (but not cancellations)
                    if *stop_reason == StopReason::Error && !cancel.is_cancelled() {
                        let err_str = error_message
                            .as_deref()
                            .unwrap_or("Unknown error")
                            .to_string();
                        tx.send(AgentEvent::Error {
                            error: AgentErrorInfo {
                                kind: AgentErrorKind::Provider,
                                message: err_str,
                            },
                        })
                        .ok();
                    }
                    // Call after_turn even on error/abort so callers tracking usage don't miss this turn
                    if let Some(ref after_turn) = config.after_turn {
                        after_turn(&context.messages, usage);
                    }
                    tx.send(AgentEvent::TurnEnd {
                        message: agent_msg,
                        tool_results: vec![],
                    })
                    .ok();
                    return;
                }
            }

            // Successful turn — reset error tracking
            consecutive_errors = 0;
            compacted_after_error = false;

            // Extract tool calls
            let tool_calls: Vec<_> = match &message {
                Message::Assistant { content, .. } => content
                    .iter()
                    .filter_map(|c| match c {
                        Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some((id.clone(), name.clone(), arguments.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };

            let has_tool_calls = !tool_calls.is_empty();
            let mut tool_results: Vec<Message> = Vec::new();

            // Doom-loop detection: if the same tool batch repeats >= threshold
            // times, skip execution and inject a steering message instead.
            if has_tool_calls {
                if let Some(intervention) = doom_detector.check(&tool_calls) {
                    for (id, name, args) in &tool_calls {
                        let result = skip_tool_call_doom_loop(id, name, args, tx);
                        let am: AgentMessage = result.clone().into();
                        context.messages.push(am.clone());
                        new_messages.push(am);
                        tool_results.push(result);
                    }
                    pending.push(intervention.steering_message);

                    // Track turn + emit TurnEnd, then continue inner loop.
                    if let Some(ref mut tracker) = tracker {
                        let turn_tokens = match &message {
                            Message::Assistant { usage, .. } => {
                                (usage.input + usage.output) as usize
                            }
                            _ => context::message_tokens(&agent_msg),
                        };
                        tracker.record_turn(turn_tokens);
                    }
                    if let Some(ref after_turn) = config.after_turn {
                        let usage = match &message {
                            Message::Assistant { usage, .. } => usage.clone(),
                            _ => Usage::default(),
                        };
                        after_turn(&context.messages, &usage);
                    }
                    tx.send(AgentEvent::TurnEnd {
                        message: agent_msg,
                        tool_results,
                    })
                    .ok();
                    continue;
                }
            }

            if has_tool_calls {
                let execution = execute_tool_calls(
                    &context.tools,
                    &tool_calls,
                    tx,
                    cancel,
                    config.get_steering_messages.as_ref(),
                    &config.tool_execution,
                    &context.cwd,
                    &context.path_guard,
                    &config.spill,
                )
                .await;

                tool_results = execution.tool_results;
                steering_after_tools = execution.steering_messages;

                for result in &tool_results {
                    let am: AgentMessage = result.clone().into();
                    context.messages.push(am.clone());
                    new_messages.push(am);
                }
            }

            // Track turn for execution limits
            if let Some(ref mut tracker) = tracker {
                let turn_tokens = match &message {
                    Message::Assistant { usage, .. } => (usage.input + usage.output) as usize,
                    _ => context::message_tokens(&agent_msg),
                };
                tracker.record_turn(turn_tokens);
            }

            // after_turn callback
            if let Some(ref after_turn) = config.after_turn {
                let usage = match &message {
                    Message::Assistant { usage, .. } => usage.clone(),
                    _ => Usage::default(),
                };
                after_turn(&context.messages, &usage);
            }

            tx.send(AgentEvent::TurnEnd {
                message: agent_msg,
                tool_results,
            })
            .ok();

            // Check steering after turn
            if let Some(steering) = steering_after_tools.take() {
                if !steering.is_empty() {
                    pending = steering;
                    continue;
                }
            }

            pending = config
                .get_steering_messages
                .as_ref()
                .map(|f| f())
                .unwrap_or_default();

            // Exit inner loop if no more tool calls and no pending messages
            if !has_tool_calls && pending.is_empty() {
                break;
            }
        }

        // Agent would stop. Check for follow-ups.
        let follow_ups = config
            .get_follow_up_messages
            .as_ref()
            .map(|f| f())
            .unwrap_or_default();

        if !follow_ups.is_empty() {
            pending = follow_ups;
            continue;
        }

        break;
    }
}
