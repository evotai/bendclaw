use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::base::truncate_chars_with_ellipsis;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::session_stream::Stream;
use crate::llm::message::ChatMessage;
use crate::llm::stream::StreamEvent;
use crate::observability::log::slog;

#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub run_id: String,
    pub active_input: String,
    pub summary: String,
    pub target_scope: Option<String>,
    pub started_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRelation {
    Append,
    Revise,
    ForkOrAsk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionOption {
    ContinueCurrent,
    CancelAndSwitch,
    AppendAsFollowup,
}

#[derive(Debug, Clone)]
pub struct PendingDecision {
    pub active_run_id: String,
    pub question_id: String,
    pub question_text: String,
    pub candidate_input: String,
    pub options: Vec<DecisionOption>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct SessionTurnState {
    pub active: Option<RunSnapshot>,
    pub pending_decision: Option<PendingDecision>,
    pub queued_followup: Option<String>,
}

pub enum SubmitTurnResult {
    Started {
        stream: Stream,
        revised_from_run_id: Option<String>,
        preamble: Option<String>,
    },
    StatusReply {
        message: String,
    },
    CancelledCurrent {
        message: String,
    },
    WaitingForDecision {
        question_id: String,
        message: String,
        options: Vec<String>,
    },
    FollowupQueued {
        message: String,
    },
    MessageInjected {
        message: String,
    },
    ContinuedCurrent {
        message: String,
    },
}

pub type TurnStateStore = Mutex<HashMap<String, SessionTurnState>>;

#[derive(Debug, Deserialize)]
struct RelationJson {
    relation: String,
    assistant_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DecisionJson {
    decision: String,
    assistant_message: Option<String>,
}

impl Runtime {
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_turn(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        input: String,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<SubmitTurnResult> {
        let session = self
            .get_or_create_session(agent_id, session_id, user_id)
            .await?;

        if let Some(pending) = self.pending_decision(session_id).await {
            if !session.is_running() && !is_explicit_decision_reply(&input) {
                slog!(info, "turn", "decision_expired",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %pending.active_run_id,
                    incoming_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
                );
                self.clear_pending_decision(session_id).await;
            } else {
                return self
                    .handle_pending_decision(
                        agent_id,
                        session_id,
                        input,
                        trace_id,
                        parent_run_id,
                        parent_trace_id,
                        origin_node_id,
                        is_remote_dispatch,
                        session,
                        pending,
                    )
                    .await;
            }
        }

        if !session.is_running() {
            return self
                .start_run(
                    agent_id,
                    session_id,
                    input,
                    trace_id,
                    parent_run_id,
                    parent_trace_id,
                    origin_node_id,
                    is_remote_dispatch,
                    session,
                    None,
                    None,
                )
                .await;
        }

        let active = self
            .active_snapshot(session_id)
            .await
            .unwrap_or_else(|| RunSnapshot {
                run_id: session
                    .current_run_id()
                    .unwrap_or_else(|| "unknown".to_string()),
                active_input: String::new(),
                summary: "the current work in this conversation".to_string(),
                target_scope: None,
                started_at: Instant::now(),
            });

        let active_elapsed_ms = active.started_at.elapsed().as_millis() as u64;
        slog!(info, "turn", "overlap_detected",
            session_id = %session_id,
            agent_id = %agent_id,
            active_run_id = %active.run_id,
            active_summary = %active.summary,
            active_elapsed_ms,
            incoming_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
        );

        match self
            .classify_turn_relation(agent_id, &active, &input)
            .await?
        {
            InterpretedRelation::Status { assistant_message } => {
                let session_info = session.info();
                let message = assistant_message
                    .unwrap_or_else(|| build_status_message(&active, &session_info));
                slog!(info, "turn", "status_requested",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %active.run_id,
                    active_summary = %active.summary,
                    active_elapsed_ms,
                );
                Ok(SubmitTurnResult::StatusReply { message })
            }
            InterpretedRelation::CancelCurrent { assistant_message } => {
                slog!(info, "turn", "cancel_requested",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %active.run_id,
                    active_summary = %active.summary,
                    active_elapsed_ms,
                    incoming_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
                );
                session.cancel_current();
                wait_until_idle(&session, Duration::from_secs(30)).await?;
                self.clear_active_turn(session_id).await;
                let message = assistant_message.unwrap_or_else(|| {
                    format!(
                        "I stopped the current work in this conversation after {}: {}.",
                        format_elapsed(active.started_at.elapsed()),
                        format_active_work(&active)
                    )
                });
                Ok(SubmitTurnResult::CancelledCurrent { message })
            }
            InterpretedRelation::Append { assistant_message } => {
                if session.inject_message(&input) {
                    let message = assistant_message.unwrap_or_else(|| {
                        format!(
                            "I am currently working on this conversation: {}. Your new message has been added to the conversation.",
                            format_active_work(&active)
                        )
                    });
                    slog!(info, "turn", "message_injected",
                        session_id = %session_id,
                        agent_id = %agent_id,
                        active_run_id = %active.run_id,
                        active_summary = %active.summary,
                        injected_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
                    );
                    Ok(SubmitTurnResult::MessageInjected { message })
                } else {
                    // Fallback: inbox full or session no longer running — queue for later.
                    let message = assistant_message.unwrap_or_else(|| {
                        format!(
                            "I am currently working on this conversation: {}. I will handle your new message right after it finishes.",
                            format_active_work(&active)
                        )
                    });
                    let mut states = self.turn_states.lock().await;
                    let state = states.entry(session_id.to_string()).or_default();
                    state.queued_followup =
                        Some(merge_followup(state.queued_followup.take(), &input));
                    slog!(info, "turn", "followup_buffered",
                        session_id = %session_id,
                        agent_id = %agent_id,
                        active_run_id = %active.run_id,
                        active_summary = %active.summary,
                        buffered_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
                    );
                    Ok(SubmitTurnResult::FollowupQueued { message })
                }
            }
            InterpretedRelation::Revise { assistant_message } => {
                let old_run_id = active.run_id.clone();
                slog!(info, "turn", "revision_requested",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %old_run_id,
                    active_summary = %active.summary,
                    replacement_preview = %truncate_chars_with_ellipsis(input.trim(), 160),
                    active_elapsed_ms,
                );
                session.cancel_current();
                wait_until_idle(&session, Duration::from_secs(30)).await?;
                let preamble = assistant_message.or_else(|| {
                    Some(format!(
                        "I was working on this conversation for {}: {}. I stopped that run and restarted with your updated request: {}",
                        format_elapsed(active.started_at.elapsed()),
                        active.summary,
                        truncate_chars_with_ellipsis(input.trim(), 160)
                    ))
                });
                self.start_run(
                    agent_id,
                    session_id,
                    input,
                    trace_id,
                    parent_run_id,
                    parent_trace_id,
                    origin_node_id,
                    is_remote_dispatch,
                    session,
                    Some(old_run_id),
                    preamble,
                )
                .await
            }
            InterpretedRelation::ForkOrAsk { assistant_message } => {
                let question_id = crate::base::new_id();
                let message = assistant_message
                    .unwrap_or_else(|| build_clarification_message(&active, &input));
                let active_run_id = active.run_id.clone();
                let pending = PendingDecision {
                    active_run_id: active_run_id.clone(),
                    question_id: question_id.clone(),
                    question_text: message.clone(),
                    candidate_input: input,
                    options: vec![
                        DecisionOption::ContinueCurrent,
                        DecisionOption::CancelAndSwitch,
                        DecisionOption::AppendAsFollowup,
                    ],
                    created_at: Instant::now(),
                };
                let mut states = self.turn_states.lock().await;
                states
                    .entry(session_id.to_string())
                    .or_default()
                    .pending_decision = Some(pending);
                slog!(info, "turn", "decision_required",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %active_run_id,
                    active_summary = %active.summary,
                    question_id = %question_id,
                    question = %message,
                );
                Ok(SubmitTurnResult::WaitingForDecision {
                    question_id,
                    message,
                    options: vec![
                        "continue".to_string(),
                        "switch".to_string(),
                        "followup".to_string(),
                    ],
                })
            }
        }
    }

    pub async fn complete_turn(&self, session_id: &str, run_id: &str) -> Option<String> {
        let mut states = self.turn_states.lock().await;
        let followup = {
            let state = states.get_mut(session_id)?;
            if state
                .active
                .as_ref()
                .is_some_and(|snapshot| snapshot.run_id == run_id)
            {
                state.active = None;
            }
            if state
                .pending_decision
                .as_ref()
                .is_some_and(|pending| pending.active_run_id == run_id)
            {
                state.pending_decision = None;
            }
            let followup = state.queued_followup.take();
            let remove_session = state.active.is_none()
                && state.pending_decision.is_none()
                && state.queued_followup.is_none();
            (followup, remove_session)
        };
        if followup.1 {
            states.remove(session_id);
        }
        followup.0
    }

    async fn active_snapshot(&self, session_id: &str) -> Option<RunSnapshot> {
        self.turn_states
            .lock()
            .await
            .get(session_id)
            .and_then(|state| state.active.clone())
    }

    async fn pending_decision(&self, session_id: &str) -> Option<PendingDecision> {
        self.turn_states
            .lock()
            .await
            .get(session_id)
            .and_then(|state| state.pending_decision.clone())
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_pending_decision(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        reply_input: String,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
        session: Arc<crate::kernel::session::Session>,
        pending: PendingDecision,
    ) -> Result<SubmitTurnResult> {
        match self
            .resolve_decision(agent_id, &pending, &reply_input)
            .await?
        {
            InterpretedDecision::ContinueCurrent { assistant_message } => {
                self.clear_pending_decision(session_id).await;
                let active_summary = self
                    .active_snapshot(session_id)
                    .await
                    .map(|snapshot| snapshot.summary)
                    .unwrap_or_else(|| "the current work in this conversation".to_string());
                slog!(info, "turn", "decision_resolved",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %pending.active_run_id,
                    decision = %"continue_current",
                    reply_preview = %truncate_chars_with_ellipsis(reply_input.trim(), 160),
                );
                Ok(SubmitTurnResult::ContinuedCurrent {
                    message: assistant_message.unwrap_or_else(|| {
                        format!(
                            "Understood. I will keep the current work in this conversation running: {}.",
                            active_summary
                        )
                    }),
                })
            }
            InterpretedDecision::AppendAsFollowup { assistant_message } => {
                let mut states = self.turn_states.lock().await;
                let state = states.entry(session_id.to_string()).or_default();
                state.pending_decision = None;
                state.queued_followup = Some(merge_followup(
                    state.queued_followup.take(),
                    &pending.candidate_input,
                ));
                slog!(info, "turn", "decision_resolved",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %pending.active_run_id,
                    decision = %"append_as_followup",
                    buffered_preview = %truncate_chars_with_ellipsis(pending.candidate_input.trim(), 160),
                    reply_preview = %truncate_chars_with_ellipsis(reply_input.trim(), 160),
                );
                Ok(SubmitTurnResult::FollowupQueued {
                    message: assistant_message.unwrap_or_else(|| {
                        "Understood. I will finish the current work in this conversation first, then handle your new message.".to_string()
                    }),
                })
            }
            InterpretedDecision::CancelAndSwitch { assistant_message } => {
                self.clear_pending_decision(session_id).await;
                slog!(info, "turn", "decision_resolved",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %pending.active_run_id,
                    decision = %"cancel_and_switch",
                    replacement_preview = %truncate_chars_with_ellipsis(pending.candidate_input.trim(), 160),
                    reply_preview = %truncate_chars_with_ellipsis(reply_input.trim(), 160),
                );
                if session.is_running() {
                    session.cancel_current();
                    wait_until_idle(&session, Duration::from_secs(30)).await?;
                }
                self.start_run(
                    agent_id,
                    session_id,
                    pending.candidate_input,
                    trace_id,
                    parent_run_id,
                    parent_trace_id,
                    origin_node_id,
                    is_remote_dispatch,
                    session,
                    Some(pending.active_run_id),
                    assistant_message.or_else(|| {
                        Some("Understood. I stopped the current work in this conversation and switched to your new request.".to_string())
                    }),
                )
                .await
            }
            InterpretedDecision::Unclear { assistant_message } => {
                slog!(info, "turn", "decision_unresolved",
                    session_id = %session_id,
                    agent_id = %agent_id,
                    active_run_id = %pending.active_run_id,
                    reply_preview = %truncate_chars_with_ellipsis(reply_input.trim(), 160),
                );
                Ok(SubmitTurnResult::WaitingForDecision {
                    question_id: pending.question_id,
                    message: assistant_message.unwrap_or(pending.question_text),
                    options: vec![
                        "continue".to_string(),
                        "switch".to_string(),
                        "followup".to_string(),
                    ],
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_run(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        input: String,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
        session: Arc<crate::kernel::session::Session>,
        revised_from_run_id: Option<String>,
        preamble: Option<String>,
    ) -> Result<SubmitTurnResult> {
        let stream = session
            .run(
                &input,
                trace_id,
                parent_run_id,
                parent_trace_id,
                origin_node_id,
                is_remote_dispatch,
            )
            .await?;
        let run_id = stream.run_id().to_string();
        let snapshot = RunSnapshot {
            run_id,
            active_input: input.clone(),
            summary: truncate_chars_with_ellipsis(input.trim(), 160),
            target_scope: extract_target_scope(&input),
            started_at: Instant::now(),
        };
        let mut states = self.turn_states.lock().await;
        let state = states.entry(session_id.to_string()).or_default();
        state.active = Some(snapshot);
        state.pending_decision = None;
        slog!(info, "turn", "started",
            session_id = %session_id,
            agent_id = %agent_id,
            run_id = %state.active.as_ref().map(|s| s.run_id.as_str()).unwrap_or(""),
            summary = %state.active.as_ref().map(|s| s.summary.as_str()).unwrap_or(""),
            target_scope = %state.active.as_ref().and_then(|s| s.target_scope.as_deref()).unwrap_or(""),
            revised_from_run_id = %revised_from_run_id.as_deref().unwrap_or(""),
        );
        Ok(SubmitTurnResult::Started {
            stream,
            revised_from_run_id,
            preamble,
        })
    }

    async fn clear_pending_decision(&self, session_id: &str) {
        let mut states = self.turn_states.lock().await;
        let mut remove_session = false;
        if let Some(state) = states.get_mut(session_id) {
            state.pending_decision = None;
            remove_session = state.active.is_none() && state.queued_followup.is_none();
        }
        if remove_session {
            states.remove(session_id);
        }
    }

    async fn clear_active_turn(&self, session_id: &str) {
        let mut states = self.turn_states.lock().await;
        if states.remove(session_id).is_some() {
            slog!(info, "turn", "cleared",
                session_id = %session_id,
            );
        }
    }

    async fn classify_turn_relation(
        &self,
        agent_id: &str,
        active: &RunSnapshot,
        candidate_input: &str,
    ) -> Result<InterpretedRelation> {
        let prompt = format!(
            "Classify how the new user message relates to the active work in this conversation.\n\
Return strict JSON only.\n\
Allowed relations:\n\
- status\n\
- cancel_current\n\
- append\n\
- revise\n\
- fork_or_ask\n\n\
Use status when the user is asking how the current work is going, whether it is still running, or what the current progress is.\n\
Use cancel_current when the user is asking to stop, cancel, abort, or end the current work.\n\
Use revise when the user changes target scope, constraints, filters, or execution boundaries of the same ongoing work.\n\
Use append when the user adds detail, output requirements, or a follow-up that should wait until the current work finishes.\n\
Use fork_or_ask when the relation is unclear or looks like a separate piece of work.\n\n\
Current work summary:\n{}\n\n\
Current target scope:\n{}\n\n\
Elapsed time on current work:\n{}\n\n\
New user message:\n{}\n\n\
JSON schema:\n\
{{\"relation\":\"status|cancel_current|append|revise|fork_or_ask\",\"assistant_message\":\"short natural language response\"}}",
            active.summary,
            active.target_scope.as_deref().unwrap_or(""),
            format_elapsed(active.started_at.elapsed()),
            candidate_input.trim()
        );

        let parsed: RelationJson = match self
            .call_json_interpreter(
                agent_id,
                "You classify user turn overlap for an active task. Output JSON only.",
                &prompt,
            )
            .await
        {
            Ok(parsed) => parsed,
            Err(_) => {
                if let Some(direct) = resolve_direct_control_relation(candidate_input) {
                    return Ok(direct);
                }
                return Ok(InterpretedRelation::ForkOrAsk {
                    assistant_message: None,
                });
            }
        };

        let relation = match parsed.relation.trim() {
            "status" => InterpretedRelation::Status {
                assistant_message: parsed.assistant_message,
            },
            "cancel_current" => InterpretedRelation::CancelCurrent {
                assistant_message: parsed.assistant_message,
            },
            "append" => InterpretedRelation::Append {
                assistant_message: parsed.assistant_message,
            },
            "revise" => InterpretedRelation::Revise {
                assistant_message: parsed.assistant_message,
            },
            _ => InterpretedRelation::ForkOrAsk {
                assistant_message: parsed.assistant_message,
            },
        };
        Ok(relation)
    }

    async fn resolve_decision(
        &self,
        agent_id: &str,
        pending: &PendingDecision,
        reply_input: &str,
    ) -> Result<InterpretedDecision> {
        let prompt = format!(
            "Resolve the user's reply to a clarification about the active work in this conversation.\n\
Return strict JSON only.\n\
Allowed decisions:\n\
- continue_current\n\
- cancel_and_switch\n\
- append_as_followup\n\
- unclear\n\n\
Original clarification:\n{}\n\n\
Candidate message waiting on decision:\n{}\n\n\
User reply:\n{}\n\n\
JSON schema:\n\
{{\"decision\":\"continue_current|cancel_and_switch|append_as_followup|unclear\",\"assistant_message\":\"short natural language response\"}}",
            pending.question_text,
            pending.candidate_input,
            reply_input.trim()
        );

        let parsed: DecisionJson = match self
            .call_json_interpreter(
                agent_id,
                "You resolve clarification replies for active work in a conversation. Output JSON only.",
                &prompt,
            )
            .await
        {
            Ok(parsed) => parsed,
            Err(_) => {
                if let Some(explicit) = resolve_explicit_decision(reply_input) {
                    return Ok(explicit);
                }
                return Ok(InterpretedDecision::Unclear {
                    assistant_message: None,
                });
            }
        };

        let decision = match parsed.decision.trim() {
            "continue_current" => InterpretedDecision::ContinueCurrent {
                assistant_message: parsed.assistant_message,
            },
            "cancel_and_switch" => InterpretedDecision::CancelAndSwitch {
                assistant_message: parsed.assistant_message,
            },
            "append_as_followup" => InterpretedDecision::AppendAsFollowup {
                assistant_message: parsed.assistant_message,
            },
            _ => InterpretedDecision::Unclear {
                assistant_message: parsed.assistant_message,
            },
        };
        Ok(decision)
    }

    async fn call_json_interpreter<T: for<'de> Deserialize<'de>>(
        &self,
        agent_id: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<T> {
        let pool = self.databases.agent_pool(agent_id)?;
        let llm = self.resolve_agent_llm(agent_id, &pool).await?;
        let model = llm.default_model().to_string();
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];
        let mut stream = llm.chat_stream(&model, &messages, &[], 0.0);
        let mut content = String::new();
        while let Some(event) = tokio_stream::StreamExt::next(&mut stream).await {
            match event {
                StreamEvent::ContentDelta(chunk) | StreamEvent::ThinkingDelta(chunk) => {
                    content.push_str(&chunk);
                }
                StreamEvent::Done { .. } => break,
                StreamEvent::Error(msg) => {
                    return Err(ErrorCode::internal(format!(
                        "control interpreter stream failed: {msg}"
                    )));
                }
                StreamEvent::ToolCallStart { .. }
                | StreamEvent::ToolCallDelta { .. }
                | StreamEvent::ToolCallEnd { .. }
                | StreamEvent::Usage(_) => {}
            }
        }
        parse_json_response(&content)
    }
}

#[derive(Debug)]
enum InterpretedRelation {
    Status { assistant_message: Option<String> },
    CancelCurrent { assistant_message: Option<String> },
    Append { assistant_message: Option<String> },
    Revise { assistant_message: Option<String> },
    ForkOrAsk { assistant_message: Option<String> },
}

#[derive(Debug)]
enum InterpretedDecision {
    ContinueCurrent { assistant_message: Option<String> },
    CancelAndSwitch { assistant_message: Option<String> },
    AppendAsFollowup { assistant_message: Option<String> },
    Unclear { assistant_message: Option<String> },
}

pub async fn wait_until_idle(
    session: &Arc<crate::kernel::session::Session>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while session.is_running() {
        if Instant::now() >= deadline {
            return Err(ErrorCode::internal(
                "timed out waiting for active run to stop".to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

pub fn merge_followup(existing: Option<String>, incoming: &str) -> String {
    match existing {
        Some(existing) if !existing.trim().is_empty() => {
            format!("{existing}\n\n{}", incoming.trim())
        }
        _ => incoming.trim().to_string(),
    }
}

pub fn build_clarification_message(active: &RunSnapshot, candidate_input: &str) -> String {
    format!(
        "I am still working on this conversation: {}. That has been running for {}. Your new message may change the scope or may be a separate request: {}. Reply with one of: continue, switch, or followup.",
        format_active_work(active),
        format_elapsed(active.started_at.elapsed()),
        truncate_chars_with_ellipsis(candidate_input.trim(), 120)
    )
}

fn format_active_work(active: &RunSnapshot) -> String {
    match active.target_scope.as_deref() {
        Some(scope) if !scope.trim().is_empty() => {
            format!("{} (scope: {})", active.summary, scope)
        }
        _ => active.summary.clone(),
    }
}

fn build_status_message(
    active: &RunSnapshot,
    session_info: &crate::kernel::session::session_manager::SessionInfo,
) -> String {
    let turn = session_info.current_turn.as_ref();
    let iteration = turn.map(|turn| turn.iteration).unwrap_or(0);
    let duration_ms = turn
        .map(|turn| turn.duration_ms)
        .unwrap_or_else(|| active.started_at.elapsed().as_millis() as u64);
    let elapsed = Duration::from_millis(duration_ms);
    let mut parts = vec![format!(
        "I am still working on this conversation: {}.",
        format_active_work(active)
    )];
    parts.push(format!("Elapsed: {}.", format_elapsed(elapsed)));
    if iteration > 0 {
        parts.push(format!("Current iteration: {iteration}."));
    }
    parts.push(
        "If you want, you can ask me to keep going, switch scope, or cancel the current work."
            .to_string(),
    );
    parts.join(" ")
}

fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn extract_target_scope(input: &str) -> Option<String> {
    let quoted: Vec<&str> = input.split('`').collect();
    if quoted.len() >= 3 {
        let values: Vec<&str> = quoted
            .iter()
            .enumerate()
            .filter_map(|(idx, part)| (idx % 2 == 1).then_some(*part))
            .collect();
        if !values.is_empty() {
            return Some(values.join(", "));
        }
    }
    None
}

fn parse_json_response<T: for<'de> Deserialize<'de>>(content: &str) -> Result<T> {
    let trimmed = content.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim())
        .and_then(|s| s.strip_suffix("```").map(str::trim))
        .unwrap_or(trimmed);
    serde_json::from_str(candidate).map_err(|e| {
        ErrorCode::internal(format!(
            "failed to parse JSON interpreter response: {e}; content={candidate}"
        ))
    })
}

fn is_explicit_decision_reply(input: &str) -> bool {
    resolve_explicit_decision(input).is_some()
}

fn resolve_explicit_decision(input: &str) -> Option<InterpretedDecision> {
    let normalized = normalize_control_input(input);
    match normalized.as_str() {
        "continue" | "goon" | "keepgoing" | "继续" | "继续吧" | "先继续" => {
            Some(InterpretedDecision::ContinueCurrent {
            assistant_message: Some(
                "Understood. I will keep the current work in this conversation running."
                    .to_string(),
            ),
        })
        }
        "switch" | "切换" | "切换吧" | "改做新的" | "换成新的" | "取消吧" | "取消" => {
            Some(InterpretedDecision::CancelAndSwitch {
            assistant_message: Some(
                "Understood. I will stop the current work in this conversation and switch to your updated request."
                    .to_string(),
            ),
        })
        }
        "followup" | "later" | "稍后" | "稍后处理" | "后面处理" => {
            Some(InterpretedDecision::AppendAsFollowup {
            assistant_message: Some(
                "Understood. I will finish the current work first, then handle your new message."
                    .to_string(),
            ),
        })
        }
        _ => None,
    }
}

fn resolve_direct_control_relation(input: &str) -> Option<InterpretedRelation> {
    let normalized = normalize_control_input(input);
    match normalized.as_str() {
        "status"
        | "progress"
        | "howisitgoing"
        | "whatstheprogress"
        | "进度如何"
        | "进度如何呢"
        | "还在执行吗"
        | "现在怎么样"
        | "现在咋样"
        | "目前咋样"
        | "目前分析的咋样了" => Some(InterpretedRelation::Status {
            assistant_message: None,
        }),
        "cancel"
        | "stop"
        | "abort"
        | "取消"
        | "取消吧"
        | "取消正在的执行"
        | "取消当前执行"
        | "停止"
        | "停下"
        | "停掉当前" => Some(InterpretedRelation::CancelCurrent {
            assistant_message: None,
        }),
        _ => None,
    }
}

fn normalize_control_input(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .filter(|ch| {
            !matches!(
                ch,
                ',' | '.' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'
            )
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn parse_relation_json(content: &str) -> Result<(String, Option<String>)> {
    let parsed: RelationJson = parse_json_response(content)?;
    Ok((parsed.relation, parsed.assistant_message))
}

pub fn parse_decision_json(content: &str) -> Result<(String, Option<String>)> {
    let parsed: DecisionJson = parse_json_response(content)?;
    Ok((parsed.decision, parsed.assistant_message))
}

pub fn explicit_decision_name(input: &str) -> Option<&'static str> {
    match resolve_explicit_decision(input) {
        Some(InterpretedDecision::ContinueCurrent { .. }) => Some("continue_current"),
        Some(InterpretedDecision::CancelAndSwitch { .. }) => Some("cancel_and_switch"),
        Some(InterpretedDecision::AppendAsFollowup { .. }) => Some("append_as_followup"),
        Some(InterpretedDecision::Unclear { .. }) => Some("unclear"),
        None => None,
    }
}
