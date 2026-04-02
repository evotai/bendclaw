//! LLM response collector — accumulates streamed output into a single response.

use crate::kernel::run::result::ContentBlock;
use crate::kernel::run::result::Usage;
use crate::llm::message::ToolCall;
use crate::llm::stream::StreamEvent;

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
    stream_event_summary: String,
    stream_event_sequence: String,
    text_fingerprint: String,
    thinking_fingerprint: String,
    tool_call_fingerprint: String,
    response_fingerprint: String,
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
            stream_event_summary: String::new(),
            stream_event_sequence: String::new(),
            text_fingerprint: String::new(),
            thinking_fingerprint: String::new(),
            tool_call_fingerprint: String::new(),
            response_fingerprint: String::new(),
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
    pub fn set_stream_event_summary(&mut self, stream_event_summary: impl Into<String>) {
        self.stream_event_summary = stream_event_summary.into();
    }
    pub fn chunk_count(&self) -> u32 {
        self.chunk_count
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn stream_event_summary(&self) -> &str {
        &self.stream_event_summary
    }
    pub fn set_debug_fingerprints(
        &mut self,
        stream_event_summary: impl Into<String>,
        stream_event_sequence: impl Into<String>,
        text_fingerprint: impl Into<String>,
        thinking_fingerprint: impl Into<String>,
        tool_call_fingerprint: impl Into<String>,
        response_fingerprint: impl Into<String>,
    ) {
        self.stream_event_summary = stream_event_summary.into();
        self.stream_event_sequence = stream_event_sequence.into();
        self.text_fingerprint = text_fingerprint.into();
        self.thinking_fingerprint = thinking_fingerprint.into();
        self.tool_call_fingerprint = tool_call_fingerprint.into();
        self.response_fingerprint = response_fingerprint.into();
    }
    pub fn stream_event_sequence(&self) -> &str {
        &self.stream_event_sequence
    }
    pub fn text_fingerprint(&self) -> &str {
        &self.text_fingerprint
    }
    pub fn thinking_fingerprint(&self) -> &str {
        &self.thinking_fingerprint
    }
    pub fn tool_call_fingerprint(&self) -> &str {
        &self.tool_call_fingerprint
    }
    pub fn response_fingerprint(&self) -> &str {
        &self.response_fingerprint
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
