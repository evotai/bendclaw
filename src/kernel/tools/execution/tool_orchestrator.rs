//! Stateless tool lifecycle orchestrator.
//!
//! Coordinates executor, recorder, emitter, and messages.
//! TurnContext passed per call — no internal mutable turn state.

use std::collections::HashMap;
use std::time::Instant;

use super::tool_events::EventEmitter;
use super::tool_executor::CallExecutor;
use super::tool_messages;
use super::tool_recorder::ExecutionRecorder;
use super::tool_result::ToolCallResult;
use crate::kernel::tools::execution::turn_context::TurnContext;
use crate::kernel::Message;
use crate::llm::message::ToolCall;

pub struct ToolOrchestrator {
    executor: CallExecutor,
    recorder: ExecutionRecorder,
    emitter: EventEmitter,
}

pub struct ToolDispatchOutput {
    pub messages: Vec<Message>,
    pub invoked_names: Vec<String>,
}

impl ToolOrchestrator {
    pub fn new(executor: CallExecutor, recorder: ExecutionRecorder, emitter: EventEmitter) -> Self {
        Self {
            executor,
            recorder,
            emitter,
        }
    }

    pub async fn dispatch(
        &mut self,
        tool_calls: &[ToolCall],
        deadline: Instant,
        tc: TurnContext,
    ) -> ToolDispatchOutput {
        let parsed = self.executor.parse_calls(tool_calls);
        let labels = self.recorder.labels(); // Arc clone, cheap
        let mut all_messages = Vec::new();

        // Phase 1: start (real-time, before execution)
        let mut spans = HashMap::new();
        for p in &parsed {
            let span = self.recorder.start_tool_span(p, &tc);
            self.emitter.tool_start(p).await;
            self.recorder.audit_tool_started(p, &tc).await;
            self.recorder.log_tool_started(p, &tc);
            all_messages.push(tool_messages::tool_started_message(p));
            spans.insert(p.call.id.clone(), span);
        }

        // Phase 2: execute (pure)
        let results = self.executor.execute_calls(&parsed, deadline).await;

        // Phase 3: end (real-time, after execution)
        for outcome in &results {
            let p = &outcome.parsed;
            let meta = outcome.result.operation();
            let (success, error_text, output) = match &outcome.result {
                ToolCallResult::Success(out, _) => (true, None, out.clone()),
                ToolCallResult::ToolError(msg, _) => {
                    (false, Some(msg.clone()), format!("Error: {msg}"))
                }
                ToolCallResult::InfraError(msg, _) => {
                    self.recorder.log_tool_infra_error(&p.call.name, msg);
                    (false, Some(msg.clone()), format!("Error: {msg}"))
                }
            };

            if let Some(span) = spans.get(&p.call.id) {
                if success {
                    self.recorder.complete_tool_span(span, p, meta).await;
                } else {
                    self.recorder
                        .fail_tool_span(span, p, meta, error_text.clone().unwrap_or_default())
                        .await;
                }
            }
            self.emitter.tool_end(outcome).await;
            self.recorder
                .audit_tool_ended(p, &tc, success, output.clone(), error_text.clone(), meta)
                .await;
            self.recorder
                .log_tool_result(p, meta, &tc, success, error_text.clone(), output.len());

            if success {
                all_messages.push(tool_messages::tool_completed_message(p, meta));
            } else {
                all_messages.push(tool_messages::tool_failed_message(
                    p,
                    meta,
                    error_text.unwrap_or_default(),
                ));
            }
            all_messages.push(tool_messages::tool_result_message(outcome, &labels));
        }

        let invoked_names = parsed.iter().map(|p| p.call.name.clone()).collect();
        ToolDispatchOutput {
            messages: all_messages,
            invoked_names,
        }
    }
}
