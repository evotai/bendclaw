//! Run-loop types: LLM response collector, loop state machine, and abort policy.

use std::time::Duration;
use std::time::Instant;

use crate::kernel::run::context::Context;
use crate::kernel::run::result::ContentBlock;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Usage;
use crate::llm::message::ToolCall;
use crate::llm::stream::StreamEvent;
use crate::llm::usage::TokenUsage;

// ── AbortPolicy ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortSignal {
    None,
    Aborted,
    Timeout,
    MaxIterations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDecision {
    pub signal: AbortSignal,
    pub reason: Option<Reason>,
}

#[derive(Debug, Clone, Copy)]
pub struct AbortPolicy {
    max_iterations: u32,
}

impl AbortPolicy {
    pub fn new(max_iterations: u32) -> Self {
        Self { max_iterations }
    }

    pub fn check(
        &self,
        cancelled: bool,
        now: Instant,
        deadline: Instant,
        iterations: u32,
    ) -> LoopDecision {
        let signal = if cancelled {
            AbortSignal::Aborted
        } else if now >= deadline {
            AbortSignal::Timeout
        } else if iterations >= self.max_iterations {
            AbortSignal::MaxIterations
        } else {
            AbortSignal::None
        };
        LoopDecision {
            reason: match signal {
                AbortSignal::None => None,
                AbortSignal::Aborted => Some(Reason::Aborted),
                AbortSignal::Timeout => Some(Reason::Timeout),
                AbortSignal::MaxIterations => Some(Reason::MaxIterations),
            },
            signal,
        }
    }

    pub fn check_cancel_or_timeout(
        &self,
        cancelled: bool,
        now: Instant,
        deadline: Instant,
    ) -> LoopDecision {
        let signal = if cancelled {
            AbortSignal::Aborted
        } else if now >= deadline {
            AbortSignal::Timeout
        } else {
            AbortSignal::None
        };
        LoopDecision {
            reason: match signal {
                AbortSignal::Aborted => Some(Reason::Aborted),
                AbortSignal::Timeout => Some(Reason::Timeout),
                _ => None,
            },
            signal,
        }
    }
}

// ── LLMResponse ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LLMResponse {
    text: String,
    thinking: String,
    tool_calls: Vec<ToolCall>,
    usage: Usage,
    finish_reason: String,
    error: Option<String>,
    ttft_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    chunk_count: u32,
    bytes: u64,
}

impl LLMResponse {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            thinking: String::new(),
            tool_calls: Vec::new(),
            usage: Usage::default(),
            finish_reason: "stop".into(),
            error: None,
            ttft_ms: None,
            provider: None,
            model: None,
            chunk_count: 0,
            bytes: 0,
        }
    }

    pub fn apply_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::ContentDelta(chunk) => self.text.push_str(&chunk),
            StreamEvent::ThinkingDelta(chunk) => self.thinking.push_str(&chunk),
            StreamEvent::ToolCallEnd {
                id,
                name,
                arguments,
                ..
            } => {
                self.tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            StreamEvent::Usage(u) => self.usage.add(&u),
            StreamEvent::Done {
                finish_reason,
                provider,
                model,
            } => {
                self.finish_reason = finish_reason;
                self.provider = provider;
                self.model = model;
            }
            StreamEvent::Error(msg) => self.error = Some(msg),
            _ => {}
        }
    }

    pub fn mark_cancelled(&mut self) {
        self.error = Some("cancelled".into());
    }

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }
    pub fn usage(&self) -> &Usage {
        &self.usage
    }
    pub fn finish_reason(&self) -> &str {
        &self.finish_reason
    }
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn ttft_ms(&self) -> Option<u64> {
        self.ttft_ms
    }
    pub fn set_ttft_ms(&mut self, ms: u64) {
        self.ttft_ms = Some(ms);
    }

    pub fn set_stream_stats(&mut self, chunk_count: u32, bytes: u64) {
        self.chunk_count = chunk_count;
        self.bytes = bytes;
    }

    pub fn chunk_count(&self) -> u32 {
        self.chunk_count
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn content_blocks(&self) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        if !self.thinking.is_empty() {
            blocks.push(ContentBlock::thinking(&self.thinking));
        }
        if !self.text.is_empty() {
            blocks.push(ContentBlock::text(&self.text));
        }
        blocks
    }

    pub fn response_preview(&self) -> String {
        let mut out = self.text.clone();
        for tc in &self.tool_calls {
            out.push_str(&format!("\n[tool_call] {}({})", tc.name, tc.arguments));
        }
        if let Some(ref err) = self.error {
            out.push_str(&format!("\n[error] {err}"));
        }
        out
    }
}

impl Default for LLMResponse {
    fn default() -> Self {
        Self::new()
    }
}

// ── RunLoopState ─────────────────────────────────────────────────────────────

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

    pub fn apply_checkpoint_usage(&mut self, usage: Option<&Usage>) {
        if let Some(u) = usage {
            self.usage.merge(u);
        }
    }

    pub fn record_final_response(&mut self, content: Vec<ContentBlock>) {
        self.final_content = content;
        self.has_tool_calls = false;
    }

    pub fn record_error(&mut self, err: &str) {
        self.record_final_response(vec![ContentBlock::text(format!("LLM error: {err}"))]);
    }

    /// Re-enter the LLM loop without tool dispatch (e.g. max_tokens continuation).
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
