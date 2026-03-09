use std::time::Instant;

use tokio_stream::StreamExt;

use super::engine::Engine;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::kernel::run::fmt::to_chat_messages;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::trace::SpanMeta;
use crate::kernel::ErrorSource;
use crate::kernel::Message;
use crate::kernel::OpType;
use crate::kernel::OperationMeta;
use crate::observability::server_log;

impl Engine {
    pub(super) async fn call_llm(
        &mut self,
        state: &mut RunLoopState,
        iteration: u32,
    ) -> (LLMResponse, Option<String>) {
        let mut chat_messages = Vec::new();
        if !self.ctx.system_prompt.is_empty() {
            chat_messages.push(
                crate::llm::message::ChatMessage::system(&*self.ctx.system_prompt)
                    .with_cache_control(),
            );
        }
        chat_messages.extend(to_chat_messages(&self.ctx.messages));

        let active_tools = self.ctx.tool_view.tool_schemas();
        let mut request_payload = self.audit_payload(iteration);
        request_payload.insert(
            "model".to_string(),
            serde_json::json!(self.ctx.model.to_string()),
        );
        request_payload.insert(
            "temperature".to_string(),
            serde_json::json!(self.ctx.temperature),
        );
        request_payload.insert(
            "tool_strategy".to_string(),
            serde_json::json!(format!("{:?}", self.ctx.tool_view.strategy())),
        );
        request_payload.insert(
            "messages".to_string(),
            serde_json::json!(chat_messages.clone()),
        );
        request_payload.insert("tools".to_string(), serde_json::json!(active_tools.clone()));
        let request_bytes = serde_json::to_vec(&request_payload)
            .map(|body| body.len() as u64)
            .unwrap_or(0);
        server_log::info(
            &self.ops_ctx(iteration),
            "llm",
            "request",
            server_log::ServerFields::default()
                .rows(chat_messages.len() as u64)
                .bytes(request_bytes)
                .attempt(iteration)
                .payload(serde_json::Value::Object(request_payload.clone())),
        );
        self.emit_audit("llm.request", request_payload).await;

        let _reasoning_tracker =
            OperationMeta::begin(OpType::Reasoning).timeout(self.ctx.max_duration);
        let stream = self.ctx.llm.chat_stream(
            &self.ctx.model,
            &chat_messages,
            &active_tools,
            self.ctx.temperature,
        );
        let llm_span = self
            .trace
            .start_span(
                "llm",
                "reasoning.turn",
                &self.loop_span_id,
                "reasoning",
                &SpanMeta::LlmTurn { iteration }.to_json(),
                "llm reasoning turn started",
            )
            .await;

        self.ctx.messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "started",
            serde_json::json!({"iteration": iteration}),
        ));

        let mut turn = self.collect_response(stream).await;
        let ttft_ms = turn.ttft_ms().unwrap_or(0);
        state.merge_usage(turn.usage());

        if ttft_ms > 0 && state.usage().ttft_ms == 0 {
            state.set_ttft(ttft_ms);
        }

        let llm_error = if turn.has_error() {
            let ms = llm_span.elapsed_ms();
            let err = turn.take_error().unwrap_or_default();
            llm_span
                .fail(
                    ms,
                    "llm_stream_error",
                    &err,
                    &SpanMeta::LlmFailed {
                        finish_reason: turn.finish_reason().to_string(),
                        error: err.clone(),
                    }
                    .to_json(),
                    "llm reasoning failed",
                )
                .await;
            self.emit(Event::ReasonError { error: err.clone() }).await;
            let mut payload = self.audit_payload(iteration);
            payload.insert(
                "model".to_string(),
                serde_json::json!(turn.model().unwrap_or(self.ctx.model.as_ref())),
            );
            payload.insert("provider".to_string(), serde_json::json!(turn.provider()));
            payload.insert(
                "finish_reason".to_string(),
                serde_json::json!(turn.finish_reason()),
            );
            payload.insert("error".to_string(), serde_json::json!(err.clone()));
            payload.insert("text".to_string(), serde_json::json!(turn.text()));
            payload.insert(
                "content_blocks".to_string(),
                serde_json::json!(turn.content_blocks()),
            );
            payload.insert(
                "tool_calls".to_string(),
                serde_json::json!(turn.tool_calls()),
            );
            payload.insert("usage".to_string(), serde_json::json!(turn.usage()));
            payload.insert("ttft_ms".to_string(), serde_json::json!(ttft_ms));
            payload.insert(
                "chunk_count".to_string(),
                serde_json::json!(turn.chunk_count() as u64),
            );
            payload.insert("bytes".to_string(), serde_json::json!(turn.bytes()));
            server_log::error(
                &self.ops_ctx(iteration),
                "llm",
                "failed",
                server_log::ServerFields::default()
                    .elapsed_ms(ms)
                    .tokens(turn.usage().total_tokens)
                    .bytes(turn.bytes())
                    .attempt(iteration)
                    .payload(serde_json::Value::Object(payload.clone())),
            );
            self.emit_audit("llm.error", payload).await;
            Some(err)
        } else {
            let ms = llm_span.elapsed_ms();
            llm_span
                .complete(
                    ms,
                    ttft_ms,
                    turn.usage().prompt_tokens,
                    turn.usage().completion_tokens,
                    0,
                    0.0,
                    &SpanMeta::LlmCompleted {
                        finish_reason: turn.finish_reason().to_string(),
                        prompt_tokens: turn.usage().prompt_tokens,
                        completion_tokens: turn.usage().completion_tokens,
                    }
                    .to_json(),
                    "llm reasoning turn completed",
                )
                .await;
            self.ctx.messages.push(Message::operation_event(
                "llm",
                "reasoning.turn",
                "completed",
                serde_json::json!({
                    "finish_reason": turn.finish_reason(),
                    "prompt_tokens": turn.usage().prompt_tokens,
                    "completion_tokens": turn.usage().completion_tokens,
                }),
            ));
            let mut payload = self.audit_payload(iteration);
            payload.insert(
                "model".to_string(),
                serde_json::json!(turn.model().unwrap_or(self.ctx.model.as_ref())),
            );
            payload.insert("provider".to_string(), serde_json::json!(turn.provider()));
            payload.insert(
                "finish_reason".to_string(),
                serde_json::json!(turn.finish_reason()),
            );
            payload.insert("text".to_string(), serde_json::json!(turn.text()));
            payload.insert(
                "content_blocks".to_string(),
                serde_json::json!(turn.content_blocks()),
            );
            payload.insert(
                "tool_calls".to_string(),
                serde_json::json!(turn.tool_calls()),
            );
            payload.insert("usage".to_string(), serde_json::json!(turn.usage()));
            payload.insert("ttft_ms".to_string(), serde_json::json!(ttft_ms));
            payload.insert(
                "chunk_count".to_string(),
                serde_json::json!(turn.chunk_count() as u64),
            );
            payload.insert("bytes".to_string(), serde_json::json!(turn.bytes()));
            server_log::info(
                &self.ops_ctx(iteration),
                "llm",
                "completed",
                server_log::ServerFields::default()
                    .elapsed_ms(ms)
                    .tokens(turn.usage().total_tokens)
                    .bytes(turn.bytes())
                    .attempt(iteration)
                    .payload(serde_json::Value::Object(payload.clone())),
            );
            self.emit_audit("llm.response", payload).await;
            None
        };

        (turn, llm_error)
    }

    pub(super) fn record_llm_error(&mut self, err: &str, turn: &LLMResponse) {
        self.ctx.messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "failed",
            serde_json::json!({"finish_reason": turn.finish_reason(), "error": err}),
        ));
        self.ctx
            .messages
            .push(Message::error(ErrorSource::Llm, err));
    }

    pub(super) fn record_assistant_message(
        &mut self,
        turn: &LLMResponse,
        state: &mut RunLoopState,
    ) {
        let ttft_ms = turn.ttft_ms().unwrap_or(0);
        let reasoning_tracker =
            OperationMeta::begin(OpType::Reasoning).timeout(self.ctx.max_duration);
        let reasoning_meta = reasoning_tracker
            .summary(format!(
                "{} -> {} tokens",
                self.ctx.model,
                turn.usage().total_tokens
            ))
            .finish();

        let msg_metrics = crate::kernel::session::message::MessageMetrics {
            input_tokens: turn.usage().prompt_tokens,
            output_tokens: turn.usage().completion_tokens,
            reasoning_tokens: 0,
            ttft_ms,
            duration_ms: 0,
        };

        if turn.has_tool_calls() {
            self.ctx.messages.push(Message::assistant_with_metrics(
                turn.text(),
                turn.tool_calls().to_vec(),
                reasoning_meta,
                msg_metrics,
            ));
        } else {
            self.ctx.messages.push(Message::assistant_with_metrics(
                turn.text(),
                Vec::new(),
                reasoning_meta,
                msg_metrics,
            ));
            state.record_final_response(turn.content_blocks());
        }
    }

    async fn collect_response(&self, stream: crate::llm::stream::ResponseStream) -> LLMResponse {
        let mut resp = LLMResponse::new();
        let mut stream = stream;
        let mut first_token_seen = false;
        let stream_start = Instant::now();
        let mut chunk_count = 0u32;
        let mut bytes = 0u64;
        loop {
            tokio::select! {
                event = stream.next() => {
                    match event {
                        Some(event) => {
                            chunk_count += 1;
                            bytes += match &event {
                                crate::llm::stream::StreamEvent::ContentDelta(chunk)
                                | crate::llm::stream::StreamEvent::ThinkingDelta(chunk)
                                | crate::llm::stream::StreamEvent::Error(chunk) => chunk.len() as u64,
                                crate::llm::stream::StreamEvent::ToolCallStart { id, name, .. } => {
                                    (id.len() + name.len()) as u64
                                }
                                crate::llm::stream::StreamEvent::ToolCallDelta { json_chunk, .. } => {
                                    json_chunk.len() as u64
                                }
                                crate::llm::stream::StreamEvent::ToolCallEnd { id, name, arguments, .. } => {
                                    (id.len() + name.len() + arguments.len()) as u64
                                }
                                crate::llm::stream::StreamEvent::Usage(_) => 0,
                                crate::llm::stream::StreamEvent::Done { finish_reason, provider, model } => {
                                    (finish_reason.len()
                                        + provider.as_ref().map_or(0, |s| s.len())
                                        + model.as_ref().map_or(0, |s| s.len())) as u64
                                }
                            };
                            if !first_token_seen {
                                if let Some(delta) = Delta::from_stream_event(&event) {
                                    if matches!(&delta, Delta::Text { .. } | Delta::Thinking { .. }) {
                                        resp.set_ttft_ms(stream_start.elapsed().as_millis() as u64);
                                        first_token_seen = true;
                                    }
                                    self.emit(Event::StreamDelta(delta)).await;
                                }
                            } else if let Some(delta) = Delta::from_stream_event(&event) {
                                self.emit(Event::StreamDelta(delta)).await;
                            }
                            resp.apply_stream_event(event);
                        }
                        None => break,
                    }
                }
                _ = self.cancel.cancelled() => {
                    tracing::info!("LLM stream cancelled");
                    resp.mark_cancelled();
                    break;
                }
            }
        }
        resp.set_stream_stats(chunk_count, bytes);
        tracing::debug!(
            tool_calls = resp.tool_calls().len(),
            prompt_tokens = resp.usage().prompt_tokens,
            completion_tokens = resp.usage().completion_tokens,
            finish_reason = %resp.finish_reason(),
            has_error = resp.has_error(),
            ttft_ms = resp.ttft_ms().unwrap_or(0),
            chunk_count,
            bytes,
            "llm response collected"
        );
        resp
    }
}
