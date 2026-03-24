use std::sync::Arc;
use std::time::Duration;

use crate::base::new_id;
use crate::base::Result;
use crate::kernel::run::event::Event;
use crate::kernel::runtime::pending_decision::clarification_template;
use crate::kernel::runtime::pending_decision::resolve_decision;
use crate::kernel::runtime::pending_decision::DecisionOption;
use crate::kernel::runtime::pending_decision::DecisionResolution;
use crate::kernel::runtime::pending_decision::PendingDecision;
use crate::kernel::runtime::turn_relation::RunSnapshot;
use crate::kernel::runtime::turn_relation::TurnRelation;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::session_stream::Stream;
use crate::kernel::session::Session;
use crate::observability::log::slog;

pub enum SubmitResult {
    Started {
        stream: Stream,
        preamble: Option<String>,
    },
    Injected,
    Queued,
    Control {
        message: String,
    },
}

impl Runtime {
    pub async fn submit_turn(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        input: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<SubmitResult> {
        let normalized = normalize_control_input(input);

        if is_cancel_command(&normalized) {
            if let Some(s) = self.sessions().get(session_id) {
                s.cancel_current();
            }
            self.turn_coordinator.remove_decision(session_id);
            return Ok(SubmitResult::Control {
                message: "Run cancelled.".to_string(),
            });
        }

        if is_status_command(&normalized) {
            let message = match self.sessions().get(session_id) {
                Some(ref s) => {
                    let info = s.info();
                    format!("status={} session={}", info.status, info.id)
                }
                None => "No active session.".to_string(),
            };
            return Ok(SubmitResult::Control { message });
        }

        let session = self
            .get_or_create_session(agent_id, session_id, user_id)
            .await?;

        // Pending decision: treat incoming message as the user's resolution reply.
        if let Some(decision) = self.turn_coordinator.get_decision(session_id) {
            return self
                .resolve_pending_decision(
                    &session,
                    decision,
                    input,
                    trace_id,
                    parent_run_id,
                    parent_trace_id,
                    origin_node_id,
                    is_remote_dispatch,
                )
                .await;
        }

        // Session idle — start a new run.
        if !session.is_running() {
            let stream = session
                .run(
                    input,
                    trace_id,
                    parent_run_id,
                    parent_trace_id,
                    origin_node_id,
                    is_remote_dispatch,
                )
                .await?;
            self.turn_coordinator.store_snapshot(
                session_id,
                RunSnapshot::from_input(session_id, stream.run_id(), input),
            );
            return Ok(SubmitResult::Started {
                stream,
                preamble: None,
            });
        }

        // Session is running — classify the relation to the active task.
        let relation = match self.turn_coordinator.get_snapshot(session_id) {
            Some(ref snap) => {
                let llm = self.llm();
                let model = self.model();
                self.turn_coordinator
                    .classifier()
                    .classify(&llm, &model, snap, input)
                    .await
            }
            None => TurnRelation::ForkOrAsk,
        };

        slog!(debug, "session_router", "overlap_classified",
            session_id,
            relation = ?relation,
        );

        match relation {
            TurnRelation::Append => {
                session.queue_followup(input.to_string());
                Ok(SubmitResult::Queued)
            }
            TurnRelation::Revise => {
                let old_run_id = session.current_run_id().unwrap_or_default();
                session.cancel_current();
                let became_idle = wait_until_idle(
                    self,
                    session_id,
                    Duration::from_millis(50),
                    Duration::from_secs(30),
                )
                .await;
                if !became_idle {
                    slog!(warn, "session_router", "revise_timeout",
                        session_id,
                        old_run_id = %old_run_id,
                    );
                }
                let mut stream = session
                    .run(
                        input,
                        trace_id,
                        parent_run_id,
                        parent_trace_id,
                        origin_node_id,
                        is_remote_dispatch,
                    )
                    .await?;
                self.turn_coordinator.store_snapshot(
                    session_id,
                    RunSnapshot::from_input(session_id, stream.run_id(), input),
                );
                slog!(info, "session_router", "task_revised",
                    session_id,
                    old_run_id = %old_run_id,
                    new_run_id = %stream.run_id(),
                );
                stream.prepend_event(Event::TaskRevised {
                    previous_run_id: old_run_id,
                    message: "Task revised: restarting with updated request.".to_string(),
                });
                Ok(SubmitResult::Started {
                    stream,
                    preamble: Some(
                        "Received. I stopped the current task and am restarting with your updated request."
                            .to_string(),
                    ),
                })
            }
            TurnRelation::ForkOrAsk => {
                let active_run_id = session.current_run_id().unwrap_or_default();
                let active_summary = self
                    .turn_coordinator
                    .get_snapshot(session_id)
                    .map(|s| s.summary.clone())
                    .unwrap_or_else(|| "the current task".to_string());
                let question_text = clarification_template(&active_summary);
                let question_id = new_id();
                let decision = PendingDecision {
                    session_id: session_id.to_string(),
                    active_run_id,
                    question_id: question_id.clone(),
                    question_text: question_text.clone(),
                    candidate_input: input.to_string(),
                    options: vec![
                        DecisionOption::ContinueCurrent,
                        DecisionOption::CancelAndSwitch,
                        DecisionOption::AppendAsFollowup,
                    ],
                    created_at: std::time::Instant::now(),
                };
                self.turn_coordinator.store_decision(decision);
                session.inject_event(Event::DecisionRequired {
                    question_id,
                    message: question_text.clone(),
                    options: vec![
                        "continue".to_string(),
                        "switch".to_string(),
                        "append".to_string(),
                    ],
                });
                Ok(SubmitResult::Control {
                    message: question_text,
                })
            }
        }
    }

    async fn resolve_pending_decision(
        self: &Arc<Self>,
        session: &Arc<Session>,
        decision: PendingDecision,
        reply: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<SubmitResult> {
        let session_id = &decision.session_id;
        let resolution = resolve_decision(reply);
        self.turn_coordinator.remove_decision(session_id);

        match resolution {
            DecisionResolution::ContinueCurrent => {
                session.queue_followup(decision.candidate_input);
                Ok(SubmitResult::Queued)
            }
            DecisionResolution::CancelAndSwitch => {
                session.cancel_current();
                let became_idle = wait_until_idle(
                    self,
                    session_id,
                    Duration::from_millis(50),
                    Duration::from_secs(30),
                )
                .await;
                if !became_idle {
                    slog!(warn, "session_router", "decision_switch_timeout",
                        session_id = %session_id,
                    );
                }
                let stream = session
                    .run(
                        &decision.candidate_input,
                        trace_id,
                        parent_run_id,
                        parent_trace_id,
                        origin_node_id,
                        is_remote_dispatch,
                    )
                    .await?;
                self.turn_coordinator.store_snapshot(
                    session_id,
                    RunSnapshot::from_input(session_id, stream.run_id(), &decision.candidate_input),
                );
                Ok(SubmitResult::Started {
                    stream,
                    preamble: Some("Switching to your new request.".to_string()),
                })
            }
            DecisionResolution::AppendAsFollowup => {
                session.queue_followup(decision.candidate_input);
                Ok(SubmitResult::Queued)
            }
        }
    }
}

/// Wait until the session becomes idle, polling at the given interval.
pub async fn wait_until_idle(
    runtime: &Arc<Runtime>,
    session_id: &str,
    poll_interval: Duration,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        match runtime.sessions().get(session_id) {
            Some(session) if !session.is_idle() => {}
            _ => return true,
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Merge a queued followup into a new run if the session is now idle.
pub async fn merge_followup(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    trace_id: &str,
) -> Option<Stream> {
    let session = runtime.sessions().get(session_id)?;
    if !session.is_idle() {
        return None;
    }
    let followup = session.take_followup()?;
    let stream = session
        .run(&followup, trace_id, None, "", "", false)
        .await
        .ok()?;
    runtime.turn_coordinator.store_snapshot(
        session_id,
        RunSnapshot::from_input(session_id, stream.run_id(), &followup),
    );
    let _ = agent_id;
    let _ = user_id;
    Some(stream)
}

fn normalize_control_input(input: &str) -> String {
    input.trim().to_lowercase()
}

fn is_cancel_command(normalized: &str) -> bool {
    matches!(normalized, "stop" | "cancel" | "abort")
}

fn is_status_command(normalized: &str) -> bool {
    matches!(normalized, "status" | "progress")
}
