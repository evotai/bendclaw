use std::sync::Arc;

use async_trait::async_trait;

use crate::conf::LlmConfig;
use crate::error::Result;
use crate::protocol::RunEvent;
use crate::protocol::TranscriptItem;

#[derive(Debug, Clone)]
pub struct Request {
    pub prompt: String,
    pub session_id: Option<String>,
    pub max_turns: Option<u32>,
    pub append_system_prompt: Option<String>,
}

impl Request {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            session_id: None,
            max_turns: None,
            append_system_prompt: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestResult {
    pub session_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone)]
pub struct RequestOptions {
    pub llm: LlmConfig,
    pub cwd: String,
    pub transcript: Vec<TranscriptItem>,
    pub prompt: String,
    pub max_turns: Option<u32>,
    pub append_system_prompt: Option<String>,
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()>;
}
