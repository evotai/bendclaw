//! Tool dispatch — thin orchestration layer.
//!
//! Engine delegates to ToolOrchestrator for the full parse → start → execute → end pipeline.

use std::sync::atomic::Ordering;

use super::super::llm::engine_state::RunLoopState;
use super::super::llm::turn_engine::Engine;
use super::super::memory::pressure;
use super::super::memory::pressure::PressureLevel;
use super::turn_context::TurnContext;
use crate::execution::event::Event;
use crate::execution::hooks::SteeringDecision;
use crate::llm::message::ToolCall;
use crate::planning::prompt_projection;

impl Engine {
    pub(in crate::execution) async fn dispatch_tools(
        &mut self,
        tool_calls: &[ToolCall],
        state: &mut RunLoopState,
    ) {
        let tc = TurnContext {
            turn: self.iteration.load(Ordering::Relaxed),
            loop_span_id: self.loop_span_id.clone(),
        };
        let output = self
            .orchestrator
            .dispatch(tool_calls, state.deadline(), tc)
            .await;

        self.ctx.messages.extend(output.messages);

        // ── steering check ──
        if let Some(ref source) = self.steering_source {
            let iteration = self.iteration.load(Ordering::Relaxed);
            if let SteeringDecision::Redirect(msgs) = source.check_steering(iteration).await {
                for msg in msgs {
                    self.emit(Event::MessageInjected {
                        content: msg.text(),
                    })
                    .await;
                    self.ctx
                        .messages
                        .push(msg.with_run_id(self.ctx.run_id.to_string()));
                }
            }
        }

        self.ctx.tool_view.note_invoked_batch(&output.invoked_names);
        self.ctx.tool_view.advance();
    }

    pub(in crate::execution) async fn try_compact(&mut self, state: &mut RunLoopState) {
        let total_tokens = prompt_projection::count_prompt_tokens(&self.ctx.messages);
        let max_tokens = state.max_context_tokens();
        let level = pressure::assess(total_tokens, max_tokens);

        if matches!(level, PressureLevel::Elevated | PressureLevel::High) {
            if let Some(ref mem) = self.memory {
                let transcript =
                    crate::execution::compaction::build_transcript_from(&self.ctx.messages);
                let result = mem
                    .extract_and_save(
                        &transcript,
                        &self.ctx.user_id,
                        &self.ctx.agent_id,
                        self.cancel.clone(),
                    )
                    .await;
                if result.facts_written > 0 {
                    self.emit(Event::MemoryExtracted {
                        facts_written: result.facts_written,
                    })
                    .await;
                }
                state.add_token_usage(&result.token_usage);
            }
        }

        if matches!(level, PressureLevel::High | PressureLevel::Critical) {
            if let Some(info) = self
                .compactor
                .compact(&mut self.ctx.messages, max_tokens, self.ctx.run_id.as_ref())
                .await
            {
                state.add_token_usage(&info.token_usage);
                self.emit(Event::CompactionDone {
                    messages_before: info.messages_before,
                    messages_after: info.messages_after,
                    summary_len: info.summary_len,
                })
                .await;
                if let Some(checkpoint) = info.checkpoint {
                    self.latest_checkpoint = Some(checkpoint);
                }
            }
        }
    }
}
