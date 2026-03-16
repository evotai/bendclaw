//! Per-session context — pure data carrier, no resources.

use std::sync::Arc;
use std::time::Duration;

use crate::kernel::runtime::agent_config::CheckpointConfig;
use crate::kernel::tools::progressive::ProgressiveToolView;
use crate::kernel::Message;
use crate::llm::provider::LLMProvider;

/// Per-session context — pure data, no sandbox or tool resources.
///
/// Built internally by `Session::chat()`.
#[allow(dead_code)]
pub(crate) struct Context {
    // ── Identity ──
    pub agent_id: Arc<str>,
    pub user_id: Arc<str>,
    pub session_id: Arc<str>,
    pub run_id: Arc<str>,
    pub turn: u32,
    pub trace_id: Arc<str>,

    // ── LLM ──
    pub llm: Arc<dyn LLMProvider>,
    pub model: Arc<str>,
    pub temperature: f64,

    // ── Limits ──
    pub max_iterations: u32,
    pub max_context_tokens: usize,
    pub max_duration: Duration,
    pub checkpoint: Arc<CheckpointConfig>,

    // ── Data ──
    pub tool_view: ProgressiveToolView,
    pub system_prompt: Arc<str>,
    pub messages: Vec<Message>,
}
