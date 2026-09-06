//! Context compaction integration with the agent loop.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::config::AgentLoopConfig;
use super::event_sink::EventSink;
use crate::context::AfterResponseAction;
use crate::context::CompactionController;
use crate::context::CompactionResponse;
use crate::context::ContextTracker;
use crate::context::ModelId;
use crate::context::SummarizerContext;
use crate::context::SummaryContexts;
use crate::context::UsageSnapshot;
use crate::types::*;

/// User-visible message emitted when overflow recovery is exhausted.
const OVERFLOW_EXHAUSTED_MESSAGE: &str =
    "Context overflow recovery failed after one compact-and-retry attempt. \
     Try reducing context or switching to a larger-context model.";

/// User-visible message emitted when overflow recovery could not compact.
const OVERFLOW_RECOVERY_FAILED_MESSAGE: &str =
    "Context overflow recovery failed: could not compact the context. \
     Try reducing context or switching to a larger-context model.";

pub(super) struct CompactionRequestShape<'a> {
    pub(super) system_prompt: &'a str,
    pub(super) tools: &'a [crate::provider::ToolDefinition],
    pub(super) prompt_cache_key: Option<&'a str>,
}

impl CompactionRequestShape<'_> {
    fn summarizer_context(&self, config: &AgentLoopConfig) -> SummarizerContext {
        SummarizerContext {
            provider: Arc::clone(&config.provider),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            thinking_level: config.thinking_level,
            system_prompt: self.system_prompt.to_string(),
            tools: self.tools.to_vec(),
            max_tokens: config.max_tokens,
            cache_config: config.cache_config.clone(),
            prompt_cache_key: self.prompt_cache_key.map(str::to_string),
            model_config: config.model_config.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CompactionCheckPhase {
    BeforePrompt,
    RunEnd,
}

pub(super) struct CompactionCheckInput<'a> {
    pub(super) assistant_message: &'a Message,
    pub(super) config: &'a AgentLoopConfig,
    pub(super) request_shape: CompactionRequestShape<'a>,
    pub(super) phase: CompactionCheckPhase,
}

/// Apply pi's single response-driven compaction policy.
///
/// Run-end checks ignore aborted responses. Before a new explicit prompt, the
/// same response is checked again with aborted usage included. Internal tool,
/// steering, retry, and follow-up turns never invoke this function.
pub(super) async fn check_compaction(
    controller: &mut Option<CompactionController>,
    tracker: &mut ContextTracker,
    messages: &mut Vec<AgentMessage>,
    input: CompactionCheckInput<'_>,
    cancel: CancellationToken,
    tx: &EventSink,
) -> bool {
    let CompactionCheckInput {
        assistant_message,
        config,
        request_shape,
        phase,
    } = input;
    if phase == CompactionCheckPhase::RunEnd
        && matches!(assistant_message, Message::Assistant {
            stop_reason: StopReason::Aborted,
            ..
        })
    {
        return false;
    }

    let ctrl = match controller.as_mut() {
        Some(ctrl) => ctrl,
        None => return false,
    };
    let usage = match usage_snapshot_from_message(assistant_message) {
        Some(usage) => usage,
        None => return false,
    };
    let current_model = ModelId {
        provider: target_provider(config)
            .unwrap_or(&usage.model.provider)
            .to_string(),
        model: config.model.clone(),
    };
    let remote_ctx = request_shape.summarizer_context(config);
    let llm_ctx = config.compaction_context.as_ref().unwrap_or(&remote_ctx);
    let contexts = SummaryContexts::separate(
        Some(&remote_ctx),
        Some(llm_ctx),
        config.compaction_fallback_context.as_ref(),
    );
    let response = ctrl
        .after_response_with_contexts(messages, &usage, &current_model, contexts, cancel.clone())
        .await;

    // Error/all-zero responses do not provide a direct context size. Estimate
    // only from the selected model's latest real usage plus trailing messages;
    // without that model-specific anchor there is no compaction decision. This
    // prevents a model switch from compacting against the old model's boundary.
    // Any response the trigger already classified (overflow, exhausted, failed
    // recovery) must not re-enter through the estimate fallback.
    let anchor_estimate = if response.action == AfterResponseAction::Continue
        && response.stats.is_none()
        && response.reason.is_none()
        && needs_usage_anchor_estimate(assistant_message)
    {
        tracker.estimate_context_tokens_from_anchor_for_model(
            messages,
            Some(&current_model.provider),
            Some(&current_model.model),
        )
    } else {
        None
    };
    let response = if let Some(estimated_tokens) = anchor_estimate {
        ctrl.compact_on_estimate_with_contexts(
            messages,
            estimated_tokens,
            &current_model,
            contexts,
            cancel,
        )
        .await
    } else {
        response
    };

    let should_retry = response.action == AfterResponseAction::Retry;
    emit_compaction_events(ctrl, tracker, messages, &response, tx).await;
    should_retry
}

/// Emit compaction lifecycle events and the overflow-exhausted notice.
///
/// Shared by post-response and pre-prompt compaction so both surface the same
/// observability events and persist via the app layer's compact orchestrator.
async fn emit_compaction_events(
    ctrl: &CompactionController,
    tracker: &mut ContextTracker,
    messages: &[AgentMessage],
    response: &CompactionResponse,
    tx: &EventSink,
) {
    if let Some(ref stats) = response.stats {
        let reason = response
            .reason
            .unwrap_or(crate::context::CompactReason::Threshold);
        let will_retry = response.action == AfterResponseAction::Retry;
        tx.send(AgentEvent::ContextCompactionStarted {
            reason,
            estimated_tokens: response.context_tokens.unwrap_or(stats.before_tokens),
            context_window: ctrl.config().displayed_window(),
            reserve_tokens: ctrl.config().reserve_tokens,
            trigger_threshold: ctrl.config().trigger_threshold(),
            will_retry,
        })
        .await
        .ok();
        tx.send(AgentEvent::ContextCompactionEnd {
            reason,
            stats: stats.clone(),
            messages: messages.to_vec(),
            state: ctrl.state().clone(),
            summary: stats.summary.clone(),
            context_window: ctrl.config().displayed_window(),
            will_retry,
        })
        .await
        .ok();
        tracker.record_compaction_done(ctrl.state().timestamp);
    }

    if response.overflow_exhausted {
        tx.send(AgentEvent::Error {
            error: AgentErrorInfo {
                kind: AgentErrorKind::Runtime,
                message: OVERFLOW_EXHAUSTED_MESSAGE.to_string(),
            },
        })
        .await
        .ok();
    }

    if response.overflow_recovery_failed {
        tx.send(AgentEvent::Error {
            error: AgentErrorInfo {
                kind: AgentErrorKind::Runtime,
                message: OVERFLOW_RECOVERY_FAILED_MESSAGE.to_string(),
            },
        })
        .await
        .ok();
    }
}

fn target_provider(config: &AgentLoopConfig) -> Option<&str> {
    config.model_config.as_ref().map(|model| model.provider())
}

/// Whether pi would estimate this response from the latest real usage anchor.
/// Overflow errors stay on the dedicated compact-and-retry path.
fn needs_usage_anchor_estimate(message: &Message) -> bool {
    matches!(
        message,
        Message::Assistant {
            stop_reason: StopReason::Error,
            error_message,
            ..
        } if !error_message
            .as_deref()
            .is_some_and(crate::provider::error::is_context_overflow_message)
    ) || matches!(
        message,
        Message::Assistant { usage, .. } if usage.context_tokens() == 0
    )
}

fn usage_snapshot_from_message(message: &Message) -> Option<UsageSnapshot> {
    if let Message::Assistant {
        usage,
        stop_reason,
        model,
        provider,
        timestamp,
        error_message,
        ..
    } = message
    {
        Some(UsageSnapshot {
            input: usage.input as usize,
            cache_read: usage.cache_read as usize,
            cache_write: usage.cache_write as usize,
            output: usage.output as usize,
            total_tokens: usage.total_tokens as usize,
            model: ModelId {
                provider: provider.clone(),
                model: model.clone(),
            },
            timestamp: *timestamp,
            stop_reason: stop_reason.clone(),
            error_message: error_message.clone(),
        })
    } else {
        None
    }
}
