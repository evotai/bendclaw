//! Invocation request types — orthogonal dimensions for a run.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::channels::model::context::ChannelContext;
use crate::sessions::runtime::run_options::RunOptions;

/// Per-invocation conversation context.
pub enum ConversationContext {
    None,
    Channel(ChannelContext),
}

/// A complete invocation request.
pub struct InvocationRequest {
    pub agent_id: String,
    pub user_id: String,
    pub context: ConversationContext,
    pub prompt: String,
    pub options: RunOptions,
    pub session_options: SessionBuildOptions,
}

/// Build-time options for the session (workspace, tool filter, LLM override).
#[derive(Default)]
pub struct SessionBuildOptions {
    pub cwd: Option<PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn crate::llm::provider::LLMProvider>>,
}
