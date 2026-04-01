//! Turn state — loop config and mutable iteration state.

use std::time::Duration;
use std::time::Instant;

use super::abort::AbortPolicy;
use super::abort::LoopDecision;
use crate::kernel::run::context::Context;
use crate::kernel::run::result::ContentBlock;
use crate::kernel::run::result::Usage;
use crate::llm::usage::TokenUsage;

#[derive(Debug, Clone, Copy)]
pub struct RunLoopConfig {
    pub max_duration: Duration,
    pub max_context_tokens: usize,
}

impl RunLoopConfig {
    pub(crate) fn from_context(ctx: &Context) -> Self {
        Self {
            max_duration: ctx.max_duration,
            max_context_tokens: ctx.max_context_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunLoopState {
    config: RunLoopConfig,
    deadline: Instant,
    iterations: u32,
    usage: Usage,
    final_content: Vec<ContentBlock>,
    has_tool_calls: bool,
    consecutive_max_tokens: u32,
    overflow_retries: u32,
}

impl RunLoopState {
    pub fn new(config: RunLoopConfig, started_at: Instant) -> Self {
        Self {
            deadline: started_at + config.max_duration,
            config,
            iterations: 0,
            usage: Usage::default(),
            final_content: Vec::new(),
            has_tool_calls: true,
            consecutive_max_tokens: 0,
            overflow_retries: 0,
        }
    }

    pub fn should_continue(&self) -> bool {
        self.has_tool_calls
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn max_context_tokens(&self) -> usize {
        self.config.max_context_tokens
    }
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    pub fn usage(&self) -> &Usage {
        &self.usage
    }
    pub fn final_content(&self) -> &[ContentBlock] {
        &self.final_content
    }
    pub fn begin_iteration(&mut self) -> u32 {
        self.iterations += 1;
        self.iterations
    }
    pub fn merge_usage(&mut self, usage: &Usage) {
        self.usage.merge(usage);
    }
    pub fn add_token_usage(&mut self, usage: &TokenUsage) {
        self.usage.add(usage);
    }
    pub fn set_ttft(&mut self, ms: u64) {
        self.usage.ttft_ms = ms;
    }
    pub fn record_final_response(&mut self, content: Vec<ContentBlock>) {
        self.final_content = content;
        self.has_tool_calls = false;
    }
    pub fn record_error(&mut self, err: &str) {
        self.record_final_response(vec![ContentBlock::text(format!("LLM error: {err}"))]);
    }
    pub fn force_continue(&mut self) {
        self.has_tool_calls = true;
    }
    pub fn increment_max_tokens_streak(&mut self) -> u32 {
        self.consecutive_max_tokens += 1;
        self.consecutive_max_tokens
    }
    pub fn reset_max_tokens_streak(&mut self) {
        self.consecutive_max_tokens = 0;
    }
    pub fn increment_overflow_retries(&mut self) -> u32 {
        self.overflow_retries += 1;
        self.overflow_retries
    }
    pub fn reset_overflow_retries(&mut self) {
        self.overflow_retries = 0;
    }
    pub fn check_abort(&self, policy: &AbortPolicy, cancelled: bool, now: Instant) -> LoopDecision {
        policy.check(cancelled, now, self.deadline, self.iterations)
    }
    pub fn check_cancel_or_timeout(
        &self,
        policy: &AbortPolicy,
        cancelled: bool,
        now: Instant,
    ) -> LoopDecision {
        policy.check_cancel_or_timeout(cancelled, now, self.deadline)
    }
    pub fn into_finish(self) -> (Vec<ContentBlock>, u32, Usage) {
        (self.final_content, self.iterations, self.usage)
    }
}

impl Default for RunLoopState {
    fn default() -> Self {
        Self::new(
            RunLoopConfig {
                max_duration: Duration::from_secs(300),
                max_context_tokens: 250_000,
            },
            Instant::now(),
        )
    }
}
