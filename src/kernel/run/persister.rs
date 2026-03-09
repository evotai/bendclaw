//! Run persistence: run status, run events, usage, traces.

use std::sync::Arc;
use std::time::Instant;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::run::result::Usage as AgentUsage;
use crate::kernel::run::usage::ModelRole;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::trace::TraceRecorder;
use crate::observability::audit;
use crate::observability::server_log;
use crate::storage::dal::run::record::RunMetrics;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;

pub(crate) struct TurnPersister {
    pub storage: Arc<AgentStore>,
    pub trace: TraceRecorder,
    pub agent_id: Arc<str>,
    pub session_id: String,
    pub run_id: String,
    pub user_id: Arc<str>,
    pub start: Instant,
}

impl TurnPersister {
    fn ops_ctx(&self, turn: u32) -> server_log::ServerCtx<'_> {
        server_log::ServerCtx::new(
            &self.trace.trace_id,
            &self.run_id,
            &self.session_id,
            &self.agent_id,
            turn,
        )
    }

    pub async fn persist_success(
        &self,
        result: AgentResult,
        provider: &str,
        model: &str,
        events: &[Event],
    ) -> Result<String> {
        let response_text = result.text();
        let duration_ms = self.start.elapsed().as_millis() as u64;

        if let Err(e) = self.record_usage(&result.usage, provider, model).await {
            tracing::warn!(error = %e, "failed to persist usage");
        }

        let metrics = RunMetrics {
            prompt_tokens: result.usage.prompt_tokens,
            completion_tokens: result.usage.completion_tokens,
            reasoning_tokens: result.usage.reasoning_tokens,
            total_tokens: result.usage.total_tokens,
            cache_read_tokens: result.usage.cache_read_tokens,
            cache_write_tokens: result.usage.cache_write_tokens,
            ttft_ms: result.usage.ttft_ms,
            duration_ms,
            cost: 0.0,
        };
        let metrics_json = serde_json::to_string(&metrics).unwrap_or_default();
        let status = status_from_reason(&result.stop_reason);
        let error = if matches!(status, RunStatus::Error) {
            response_text.clone()
        } else {
            String::new()
        };

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(result.iterations));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert("status".to_string(), serde_json::json!(status.as_str()));
        payload.insert("provider".to_string(), serde_json::json!(provider));
        payload.insert("model".to_string(), serde_json::json!(model));
        payload.insert(
            "iterations".to_string(),
            serde_json::json!(result.iterations),
        );
        payload.insert(
            "stop_reason".to_string(),
            serde_json::json!(result.stop_reason.as_str()),
        );
        payload.insert(
            "output".to_string(),
            serde_json::json!(response_text.clone()),
        );
        payload.insert("error".to_string(), serde_json::json!(error.clone()));
        payload.insert("usage".to_string(), serde_json::json!(result.usage.clone()));
        payload.insert("metrics".to_string(), serde_json::json!(metrics.clone()));
        payload.insert(
            "content".to_string(),
            serde_json::json!(result.content.clone()),
        );
        payload.insert(
            "messages".to_string(),
            serde_json::json!(result.messages.clone()),
        );
        all_events.push(audit::event_from_map("run.completed", payload));
        if let Err(e) = self.persist_events(&all_events).await {
            tracing::error!(error = %e, "failed to persist run events");
        }

        if let Err(e) = self
            .storage
            .run_update_final(
                &self.run_id,
                status.clone(),
                &response_text,
                &error,
                &metrics_json,
                result.stop_reason.as_str(),
                result.iterations,
            )
            .await
        {
            tracing::error!(error = %e, "failed to update run record");
        }

        match status {
            RunStatus::Completed | RunStatus::Paused => {
                let _ = self
                    .trace
                    .complete_trace(
                        duration_ms,
                        result.usage.prompt_tokens,
                        result.usage.completion_tokens,
                        0.0,
                    )
                    .await;
            }
            RunStatus::Cancelled | RunStatus::Error | RunStatus::Pending | RunStatus::Running => {
                let _ = self.trace.fail_trace(duration_ms).await;
            }
        }

        server_log::info(
            &self.ops_ctx(result.iterations),
            "run",
            "completed",
            server_log::ServerFields::default()
                .elapsed_ms(duration_ms)
                .tokens(result.usage.total_tokens)
                .bytes(metrics_json.len() as u64)
                .detail("status", status.as_str())
                .detail("provider", provider)
                .detail("model", model)
                .detail("iterations", result.iterations)
                .detail("stop_reason", result.stop_reason.as_str())
                .detail("event_count", all_events.len())
                .detail("usage", result.usage.clone())
                .detail("metrics", metrics.clone())
                .detail("response", response_text.clone())
                .detail("error", error.clone()),
        );
        Ok(response_text)
    }

    pub async fn persist_error(&self, error: &ErrorCode, events: &[Event]) {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let error_str = format!("{error}");
        tracing::error!(
            agent_id = %self.agent_id,
            session_id = %self.session_id,
            run_id = %self.run_id,
            duration_ms,
            error = %error_str,
            "run failed"
        );

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(0));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert(
            "status".to_string(),
            serde_json::json!(RunStatus::Error.as_str()),
        );
        payload.insert("error".to_string(), serde_json::json!(error_str.clone()));
        all_events.push(audit::event_from_map("run.failed", payload));
        if let Err(e) = self.persist_events(&all_events).await {
            tracing::error!(error = %e, "failed to persist run events");
        }

        server_log::error(
            &self.ops_ctx(0),
            "run",
            "failed",
            server_log::ServerFields::default()
                .elapsed_ms(duration_ms)
                .detail("status", RunStatus::Error.as_str())
                .detail("error", error_str.clone())
                .detail("event_count", all_events.len()),
        );

        let _ = self
            .storage
            .run_update_final(
                &self.run_id,
                RunStatus::Error,
                "",
                &error_str,
                "",
                Reason::Error.as_str(),
                0,
            )
            .await;
        let _ = self.trace.fail_trace(duration_ms).await;
    }

    pub async fn persist_cancelled(&self, events: &[Event]) {
        let duration_ms = self.start.elapsed().as_millis() as u64;

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(0));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert(
            "status".to_string(),
            serde_json::json!(RunStatus::Cancelled.as_str()),
        );
        payload.insert("error".to_string(), serde_json::json!("cancelled"));
        all_events.push(audit::event_from_map("run.cancelled", payload));
        if let Err(e) = self.persist_events(&all_events).await {
            tracing::error!(error = %e, "failed to persist run events");
        }

        server_log::warn(
            &self.ops_ctx(0),
            "run",
            "cancelled",
            server_log::ServerFields::default()
                .elapsed_ms(duration_ms)
                .detail("status", RunStatus::Cancelled.as_str())
                .detail("error", "cancelled")
                .detail("event_count", all_events.len()),
        );

        let _ = self
            .storage
            .run_update_status(&self.run_id, RunStatus::Cancelled)
            .await;
        let _ = self.trace.fail_trace(duration_ms).await;
    }

    async fn persist_events(&self, events: &[Event]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let records: Vec<RunEventRecord> = events
            .iter()
            .enumerate()
            .map(|(idx, event)| RunEventRecord {
                id: crate::kernel::new_id(),
                run_id: self.run_id.clone(),
                session_id: self.session_id.clone(),
                agent_id: self.agent_id.to_string(),
                user_id: self.user_id.to_string(),
                seq: (idx + 1) as u32,
                event: event.name(),
                payload: serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string()),
                created_at: String::new(),
            })
            .collect();
        let result = self.storage.run_events_insert_batch(&records).await;
        match &result {
            Ok(_) => server_log::info(
                &self.ops_ctx(0),
                "persist",
                "run_events_saved",
                server_log::ServerFields::default()
                    .rows(records.len() as u64)
                    .detail(
                        "events",
                        records
                            .iter()
                            .map(|record| record.event.clone())
                            .collect::<Vec<_>>(),
                    ),
            ),
            Err(error) => server_log::error(
                &self.ops_ctx(0),
                "persist",
                "run_events_failed",
                server_log::ServerFields::default()
                    .rows(records.len() as u64)
                    .detail(
                        "events",
                        records
                            .iter()
                            .map(|record| record.event.clone())
                            .collect::<Vec<_>>(),
                    )
                    .detail("error", error.to_string()),
            ),
        }
        result
    }

    async fn record_usage(&self, usage: &AgentUsage, provider: &str, model: &str) -> Result<()> {
        if usage.total_tokens == 0 {
            return Ok(());
        }
        let event = UsageEvent {
            agent_id: self.agent_id.to_string(),
            user_id: self.user_id.to_string(),
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            provider: provider.to_string(),
            model: model.to_string(),
            model_role: ModelRole::Reasoning,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            ttft_ms: usage.ttft_ms,
            cost: 0.0,
        };
        self.storage.usage_record(event.clone()).await?;
        server_log::info(
            &self.ops_ctx(0),
            "persist",
            "usage_recorded",
            server_log::ServerFields::default()
                .tokens(usage.total_tokens)
                .detail("provider", provider)
                .detail("model", model)
                .detail("usage", event.clone()),
        );
        let result = self.storage.usage_flush().await;
        match &result {
            Ok(_) => server_log::info(
                &self.ops_ctx(0),
                "persist",
                "usage_flushed",
                server_log::ServerFields::default()
                    .tokens(usage.total_tokens)
                    .detail("provider", provider)
                    .detail("model", model),
            ),
            Err(error) => server_log::error(
                &self.ops_ctx(0),
                "persist",
                "usage_flush_failed",
                server_log::ServerFields::default()
                    .tokens(usage.total_tokens)
                    .detail("provider", provider)
                    .detail("model", model)
                    .detail("error", error.to_string()),
            ),
        }
        result
    }
}

fn status_from_reason(reason: &Reason) -> RunStatus {
    match reason {
        Reason::EndTurn => RunStatus::Completed,
        Reason::MaxIterations | Reason::Timeout => RunStatus::Paused,
        Reason::Aborted => RunStatus::Cancelled,
        Reason::Error => RunStatus::Error,
    }
}
